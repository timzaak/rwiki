//! Scenario tests for RAG search filtering by document publish status
//! and RRF fusion correctness.
//!
//! Covers:
//! - search() and get_neighbor_chunks() only return chunks from published documents.
//!   Chunks from draft, processing, and failed documents are excluded from search results.
//! - rrf_fuse pure function: multi-list fusion, dedup, score computation, truncation.
//!
//! User Stories: US-CORE-002, US-CORE-008, US-CORE-009, US-CORE-019, US-CORE-022
//!
//! DEFERRED: search_multi_query integration tests require a working embedding API
//! (the dummy in-memory store cannot produce real embeddings). The rrf_fuse pure
//! function tests below cover the core algorithm logic. Full integration tests for
//! search_multi_query (multi-query dispatch, embedding, parallel search, window
//! expansion) should be added in the accept slot or a manual integration environment.

use std::sync::Arc;

use super::embedding_model::AppEmbeddingModel;
use super::vector_store::rrf_fuse;
use super::vector_store::SearchResult;
use super::vector_store::VectorStoreManager;
use rig::client::EmbeddingsClient;
use std::sync::Once;

static SQLITE_VEC_INIT: Once = Once::new();

fn ensure_sqlite_vec_loaded() {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

/// Create a VectorStoreManager backed by in-memory SQLite with all migrations
/// and sqlite-vec extension, but without loading any embedding model.
/// Uses a dummy OpenAI client -- the embedding model is never called in these tests,
/// since get_neighbor_chunks is pure SQL.
fn make_sql_only_store() -> VectorStoreManager {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory SQLite should open");
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL mode should be settable");
    super::migration::migrations(1536) // matches text-embedding-3-small default
        .to_latest(&mut conn)
        .expect("migrations should run");

    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Dummy OpenAI embedding model -- never invoked by get_neighbor_chunks tests.
    let client = rig::providers::openai::Client::builder()
        .api_key("test-key-unused")
        .build()
        .expect("dummy OpenAI client should build without network");
    let dummy_model = client.embedding_model("text-embedding-3-small");

    VectorStoreManager::new(
        sqlite,
        AppEmbeddingModel::new(dummy_model),
        "test-dummy".to_string(),
    )
}

/// Insert a test document row into the documents table.
async fn insert_test_document(store: &VectorStoreManager, document_id: &str, status: &str) {
    let doc_id = document_id.to_string();
    let status_val = status.to_string();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count) VALUES (?, 'test.xlsx', ?, 1)",
                rusqlite::params![doc_id, status_val],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert_test_document should succeed");
}

/// Insert a test chunk directly into chunk_metadata + vec_chunks (zero vector matching migration dimensions).
async fn insert_test_chunk(
    store: &VectorStoreManager,
    document_id: &str,
    chunk_id: &str,
    content: &str,
    page_id: &str,
    sub_index: Option<i64>,
    chunk_count: Option<i64>,
) {
    let doc_id = document_id.to_string();
    let cid = chunk_id.to_string();
    let c = content.to_string();
    let pid = page_id.to_string();
    let sub = sub_index;
    let cc = chunk_count;
    let ndims = store.ndims();

    store
        .conn
        .call(move |conn| {
            let dummy_embedding = vec![0u8; ndims * 4];

            conn.execute(
                "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) \
                 VALUES (?, ?, ?, '', NULL, NULL, '', NULL, ?, ?, ?, NULL, NULL)",
                rusqlite::params![doc_id, cid, c, pid, sub, cc],
            )?;

            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
                rusqlite::params![rowid, dummy_embedding],
            )?;

            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert_test_chunk should succeed");
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: get_neighbor_chunks() only returns chunks from published documents.
//         When chunks exist for both published and draft documents,
//         only the published document's chunks are returned.
//         This verifies the core SQL JOIN filter on documents.status = 'published'.
#[tokio::test]
async fn get_neighbor_chunks_returns_only_published_chunks() {
    let store = make_sql_only_store();

    // Published document with neighbor chunks
    insert_test_document(&store, "doc_published", "published").await;
    insert_test_chunk(
        &store,
        "doc_published",
        "pub_0",
        "published content 0",
        "page_pub",
        Some(0),
        Some(3),
    )
    .await;
    insert_test_chunk(
        &store,
        "doc_published",
        "pub_1",
        "published content 1",
        "page_pub",
        Some(1),
        Some(3),
    )
    .await;
    insert_test_chunk(
        &store,
        "doc_published",
        "pub_2",
        "published content 2",
        "page_pub",
        Some(2),
        Some(3),
    )
    .await;

    let results = store
        .get_neighbor_chunks("page_pub", 0, 3)
        .await
        .expect("get_neighbor_chunks should succeed");

    assert_eq!(results.len(), 3, "published doc should return 3 chunks");
    assert_eq!(results[0].chunk_id, "pub_0");
    assert_eq!(results[1].chunk_id, "pub_1");
    assert_eq!(results[2].chunk_id, "pub_2");
}

// User Story: US-CORE-002
// Covers: get_neighbor_chunks() excludes chunks from draft documents.
//         A document with status "draft" has its chunks excluded even though
//         the chunks exist in chunk_metadata. The SQL JOIN filter ensures
//         draft content is never surfaced to users asking questions.
#[tokio::test]
async fn get_neighbor_chunks_excludes_draft_documents() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_draft", "draft").await;
    insert_test_chunk(
        &store,
        "doc_draft",
        "draft_chunk",
        "draft content",
        "page_draft",
        Some(0),
        Some(1),
    )
    .await;

    let results = store
        .get_neighbor_chunks("page_draft", 0, 1)
        .await
        .expect("get_neighbor_chunks should succeed");

    assert!(results.is_empty(), "draft document should return 0 chunks");
}

// User Story: US-CORE-002
// Covers: get_neighbor_chunks() excludes chunks from processing documents.
//         A document with status "processing" is mid-indexing and must not
//         appear in search results until indexing completes and status changes
//         to "published".
#[tokio::test]
async fn get_neighbor_chunks_excludes_processing_documents() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_processing", "processing").await;
    insert_test_chunk(
        &store,
        "doc_processing",
        "proc_chunk",
        "processing content",
        "page_proc",
        Some(0),
        Some(1),
    )
    .await;

    let results = store
        .get_neighbor_chunks("page_proc", 0, 1)
        .await
        .expect("get_neighbor_chunks should succeed");

    assert!(
        results.is_empty(),
        "processing document should return 0 chunks"
    );
}

// User Story: US-CORE-002
// Covers: get_neighbor_chunks() excludes chunks from failed documents.
//         A document with status "failed" indicates indexing error and its
//         partial chunks must not appear in search results.
#[tokio::test]
async fn get_neighbor_chunks_excludes_failed_documents() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_failed", "failed").await;
    insert_test_chunk(
        &store,
        "doc_failed",
        "fail_chunk",
        "failed content",
        "page_fail",
        Some(0),
        Some(1),
    )
    .await;

    let results = store
        .get_neighbor_chunks("page_fail", 0, 1)
        .await
        .expect("get_neighbor_chunks should succeed");

    assert!(results.is_empty(), "failed document should return 0 chunks");
}

// User Story: US-CORE-008
// Covers: get_neighbor_chunks() filters by published status across multiple documents.
//         When requesting neighbor chunks, only published documents contribute results.
//         Draft and published documents coexist in chunk_metadata but the JOIN
//         filter ensures only published content is returned. This is the same SQL
//         filter used by search(), verified here without requiring an embedding API.
#[tokio::test]
async fn get_neighbor_chunks_filters_mixed_status_across_documents() {
    let store = make_sql_only_store();

    // Published document with chunks
    insert_test_document(&store, "doc_pub", "published").await;
    insert_test_chunk(
        &store,
        "doc_pub",
        "pub_n0",
        "published neighbor 0",
        "page_pub",
        Some(0),
        Some(3),
    )
    .await;
    insert_test_chunk(
        &store,
        "doc_pub",
        "pub_n1",
        "published neighbor 1",
        "page_pub",
        Some(1),
        Some(3),
    )
    .await;

    // Draft document with chunks
    insert_test_document(&store, "doc_drf", "draft").await;
    insert_test_chunk(
        &store,
        "doc_drf",
        "drf_n0",
        "draft neighbor 0",
        "page_drf",
        Some(0),
        Some(2),
    )
    .await;

    // Published doc returns neighbors
    let pub_results = store
        .get_neighbor_chunks("page_pub", 0, 2)
        .await
        .expect("should succeed");
    assert_eq!(
        pub_results.len(),
        2,
        "published doc should return 2 neighbors"
    );

    // Draft doc returns no neighbors (filtered by status)
    let drf_results = store
        .get_neighbor_chunks("page_drf", 0, 1)
        .await
        .expect("should succeed");
    assert!(
        drf_results.is_empty(),
        "draft doc should return 0 neighbors"
    );
}

// User Story: US-CORE-009
// Covers: Publishing a document makes its chunks searchable via get_neighbor_chunks().
//         Status change from draft to published causes chunks to appear.
//         This tests the lifecycle: draft (not searchable) -> published (searchable).
//         The SQL JOIN filter re-evaluates on each query, so status changes take
//         effect immediately without re-indexing.
#[tokio::test]
async fn publishing_document_makes_chunks_searchable() {
    let store = make_sql_only_store();

    // Insert as draft with chunks
    insert_test_document(&store, "doc_lifecycle", "draft").await;
    insert_test_chunk(
        &store,
        "doc_lifecycle",
        "lc_chunk_0",
        "lifecycle content 0",
        "page_lc",
        Some(0),
        Some(2),
    )
    .await;
    insert_test_chunk(
        &store,
        "doc_lifecycle",
        "lc_chunk_1",
        "lifecycle content 1",
        "page_lc",
        Some(1),
        Some(2),
    )
    .await;

    // Verify no results while draft
    let draft_results = store
        .get_neighbor_chunks("page_lc", 0, 2)
        .await
        .expect("should succeed");
    assert!(
        draft_results.is_empty(),
        "draft document should return 0 chunks"
    );

    // Update status to published
    let doc_id = "doc_lifecycle".to_string();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ?",
                rusqlite::params![doc_id],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("update status should succeed");

    // Verify chunks now appear after publishing
    let published_results = store
        .get_neighbor_chunks("page_lc", 0, 2)
        .await
        .expect("should succeed");
    assert_eq!(
        published_results.len(),
        2,
        "published document should return 2 chunks"
    );
    assert_eq!(published_results[0].chunk_id, "lc_chunk_0");
    assert_eq!(published_results[1].chunk_id, "lc_chunk_1");
}

// ---------------------------------------------------------------------------
// RRF fusion scenario tests
// ---------------------------------------------------------------------------

/// Helper to construct a SearchResult for rrf_fuse pure function tests.
/// These tests exercise rrf_fuse directly and need no database.
fn make_rrf_search_result(
    chunk_id: &str,
    content: &str,
    score: f64,
    page_id: &str,
) -> SearchResult {
    SearchResult {
        chunk_id: chunk_id.to_string(),
        content: content.to_string(),
        score,
        document_id: "test-doc".to_string(),
        page_id: page_id.to_string(),
        sub_index: None,
        chunk_count: None,
        title: "Test Title".to_string(),
        locale: None,
        link: None,
        tags: vec![],
        section: None,
    }
}

// User Story: US-CORE-019
// Covers: When only one query produces results, RRF fusion should return results
//         in the same order as input (rank-ordered pass-through). The RRF scores
//         should follow the formula 1/(k + rank + 1) for each position.
//         This validates the single-query pass-through path in search_multi_query,
//         where no actual fusion is needed but the scoring must still be consistent.
#[test]
fn rrf_fuse_single_list_preserves_order_with_correct_scores() {
    let list = vec![
        make_rrf_search_result("a", "content a", 0.9, "p1"),
        make_rrf_search_result("b", "content b", 0.8, "p1"),
        make_rrf_search_result("c", "content c", 0.7, "p1"),
    ];
    let result = rrf_fuse(&[list], 60, 5);

    assert_eq!(result.len(), 3, "single list should return all 3 items");
    assert_eq!(result[0].chunk_id, "a", "rank 0 should be first");
    assert_eq!(result[1].chunk_id, "b", "rank 1 should be second");
    assert_eq!(result[2].chunk_id, "c", "rank 2 should be third");

    // Verify RRF scores: 1/(k+1), 1/(k+2), 1/(k+3) with k=60
    let expected_a = 1.0 / (60.0 + 0.0 + 1.0); // rank 0: 1/61
    let expected_b = 1.0 / (60.0 + 1.0 + 1.0); // rank 1: 1/62
    let expected_c = 1.0 / (60.0 + 2.0 + 1.0); // rank 2: 1/63
    assert!(
        (result[0].score - expected_a).abs() < 1e-12,
        "rank 0 RRF score should be 1/61, got {}",
        result[0].score
    );
    assert!(
        (result[1].score - expected_b).abs() < 1e-12,
        "rank 1 RRF score should be 1/62, got {}",
        result[1].score
    );
    assert!(
        (result[2].score - expected_c).abs() < 1e-12,
        "rank 2 RRF score should be 1/63, got {}",
        result[2].score
    );
}

// User Story: US-CORE-019
// Covers: Core RRF behavior -- when two queries return overlapping chunks,
//         the shared chunk gets a higher combined score. chunk_id deduplication
//         works correctly: the chunk appearing in both lists accumulates scores
//         from both, ensuring the most broadly relevant results surface first.
//         This is the primary scenario for multi-query search quality.
#[test]
fn rrf_fuse_two_overlapping_lists_dedup_with_combined_scores() {
    let list1 = vec![
        make_rrf_search_result("A", "content A", 0.9, "p1"),
        make_rrf_search_result("B", "content B", 0.8, "p1"),
        make_rrf_search_result("C", "content C", 0.7, "p1"),
    ];
    let list2 = vec![
        make_rrf_search_result("B", "content B", 0.95, "p1"), // overlaps with list1
        make_rrf_search_result("D", "content D", 0.85, "p2"),
    ];
    let result = rrf_fuse(&[list1, list2], 60, 10);

    assert_eq!(
        result.len(),
        4,
        "should have 4 unique chunk_ids (B deduped)"
    );

    // B appears at rank 1 in list1 and rank 0 in list2
    // B's combined score: 1/(60+1+1) + 1/(60+0+1) = 1/62 + 1/61
    let expected_b = 1.0 / 62.0 + 1.0 / 61.0;
    assert_eq!(
        result[0].chunk_id, "B",
        "B should rank first (highest combined score)"
    );
    assert!(
        (result[0].score - expected_b).abs() < 1e-12,
        "B's RRF score should be 1/62 + 1/61 = {}, got {}",
        expected_b,
        result[0].score
    );

    // A appears at rank 0 in list1 only: score = 1/(60+0+1) = 1/61
    assert_eq!(result[1].chunk_id, "A", "A should rank second");

    // Verify no duplicate chunk_ids in output
    let ids: Vec<&str> = result.iter().map(|r| r.chunk_id.as_str()).collect();
    let unique_ids: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique_ids.len(),
        "no duplicate chunk_ids in output"
    );
}

// User Story: US-CORE-022
// Covers: When all queries produce no results, the fusion output is empty.
//         This validates the edge case where the outer result_lists slice itself
//         is empty (no query lists at all), ensuring no panic or incorrect output.
#[test]
fn rrf_fuse_empty_input_lists_return_empty() {
    let result = rrf_fuse(&[], 60, 10);
    assert!(
        result.is_empty(),
        "empty input slice should return empty output"
    );
}

// User Story: US-CORE-019
// Covers: When combined results exceed top_k, only the top_k highest-scoring results
//         are returned. This ensures the output size is bounded even when multiple
//         queries each contribute many results, preventing context overflow downstream.
#[test]
fn rrf_fuse_results_truncated_to_top_k() {
    let list1: Vec<SearchResult> = (0..5)
        .map(|i| {
            make_rrf_search_result(
                &format!("a{}", i),
                &format!("content a{}", i),
                0.9 - i as f64 * 0.1,
                "p1",
            )
        })
        .collect();
    let list2: Vec<SearchResult> = (0..5)
        .map(|i| {
            make_rrf_search_result(
                &format!("b{}", i),
                &format!("content b{}", i),
                0.85 - i as f64 * 0.1,
                "p2",
            )
        })
        .collect();

    let result = rrf_fuse(&[list1, list2], 60, 3);

    assert_eq!(result.len(), 3, "should truncate to top_k=3");
    // Verify descending score order
    for window in result.windows(2) {
        assert!(
            window[0].score >= window[1].score,
            "results should be sorted by descending RRF score: {} >= {}",
            window[0].score,
            window[1].score
        );
    }
}

// User Story: US-CORE-019
// Covers: Verify the RRF formula score = sum over all lists of 1/(k + rank + 1)
//         for each chunk_id. Uses a 3-list input with partial overlaps to exercise
//         multi-list accumulation. This is the definitive correctness test for the
//         core algorithm: any deviation in the formula breaks multi-query search quality.
#[test]
fn rrf_fuse_score_computation_correct_for_three_lists() {
    // List 1: X at rank 0, Y at rank 1
    let list1 = vec![
        make_rrf_search_result("X", "content X", 0.95, "p1"),
        make_rrf_search_result("Y", "content Y", 0.85, "p1"),
    ];
    // List 2: Y at rank 0, Z at rank 1
    let list2 = vec![
        make_rrf_search_result("Y", "content Y", 0.90, "p2"),
        make_rrf_search_result("Z", "content Z", 0.80, "p2"),
    ];
    // List 3: X at rank 0, Z at rank 1, W at rank 2
    let list3 = vec![
        make_rrf_search_result("X", "content X", 0.88, "p1"),
        make_rrf_search_result("Z", "content Z", 0.75, "p3"),
        make_rrf_search_result("W", "content W", 0.60, "p3"),
    ];

    let result = rrf_fuse(&[list1, list2, list3], 60, 10);
    let k = 60.0_f64;

    // Manually compute expected RRF scores:
    // X: rank 0 in list1 + rank 0 in list3 = 1/(60+0+1) + 1/(60+0+1) = 2/61
    let expected_x = 1.0 / (k + 0.0 + 1.0) + 1.0 / (k + 0.0 + 1.0);
    // Y: rank 1 in list1 + rank 0 in list2 = 1/(60+1+1) + 1/(60+0+1) = 1/62 + 1/61
    let expected_y = 1.0 / (k + 1.0 + 1.0) + 1.0 / (k + 0.0 + 1.0);
    // Z: rank 1 in list2 + rank 1 in list3 = 1/(60+1+1) + 1/(60+1+1) = 2/62
    let expected_z = 1.0 / (k + 1.0 + 1.0) + 1.0 / (k + 1.0 + 1.0);
    // W: rank 2 in list3 = 1/(60+2+1) = 1/63
    let expected_w = 1.0 / (k + 2.0 + 1.0);

    assert_eq!(result.len(), 4, "should return all 4 unique chunks");

    // Verify each chunk's score within floating point tolerance
    let tolerance = 1e-12;
    for r in &result {
        let expected = match r.chunk_id.as_str() {
            "X" => expected_x,
            "Y" => expected_y,
            "Z" => expected_z,
            "W" => expected_w,
            other => panic!("unexpected chunk_id: {other}"),
        };
        assert!(
            (r.score - expected).abs() < tolerance,
            "RRF score for {} should be {}, got {}",
            r.chunk_id,
            expected,
            r.score
        );
    }

    // Expected ranking order: X (2/61) > Y (1/61+1/62) > Z (2/62) > W (1/63)
    assert_eq!(result[0].chunk_id, "X", "X should rank first");
    assert_eq!(result[1].chunk_id, "Y", "Y should rank second");
    assert_eq!(result[2].chunk_id, "Z", "Z should rank third");
    assert_eq!(result[3].chunk_id, "W", "W should rank fourth");
}

// User Story: US-CORE-022
// Covers: result_lists = [[], []] -- all queries returned empty results.
//         This validates that rrf_fuse handles inner empty lists without panic,
//         producing an empty output. This can occur when all queries match nothing
//         in the vector store.
#[test]
fn rrf_fuse_all_empty_inner_lists_return_empty() {
    let result = rrf_fuse(&[vec![], vec![]], 60, 10);
    assert!(
        result.is_empty(),
        "all empty inner lists should return empty output"
    );
}

// User Story: US-CORE-019
// Covers: When some queries produce results and others don't, the fusion uses
//         only the available results. This models the real-world scenario where
//         one rewrite query returns matches but another returns nothing. The
//         non-empty list's results should appear with correct single-list RRF scores.
#[test]
fn rrf_fuse_one_empty_one_nonempty_returns_nonempty_results() {
    let list2 = vec![
        make_rrf_search_result("A", "content A", 0.9, "p1"),
        make_rrf_search_result("B", "content B", 0.8, "p1"),
        make_rrf_search_result("C", "content C", 0.7, "p1"),
    ];
    let result = rrf_fuse(&[vec![], list2], 60, 10);

    assert_eq!(
        result.len(),
        3,
        "should return the 3 results from the non-empty list"
    );

    // Verify correct RRF scores from the single non-empty list
    let expected_a = 1.0 / (60.0 + 0.0 + 1.0); // rank 0: 1/61
    let expected_b = 1.0 / (60.0 + 1.0 + 1.0); // rank 1: 1/62
    let expected_c = 1.0 / (60.0 + 2.0 + 1.0); // rank 2: 1/63

    assert_eq!(
        result[0].chunk_id, "A",
        "rank 0 from non-empty list should be first"
    );
    assert!(
        (result[0].score - expected_a).abs() < 1e-12,
        "A score should be 1/61, got {}",
        result[0].score
    );
    assert_eq!(
        result[1].chunk_id, "B",
        "rank 1 from non-empty list should be second"
    );
    assert!(
        (result[1].score - expected_b).abs() < 1e-12,
        "B score should be 1/62, got {}",
        result[1].score
    );
    assert_eq!(
        result[2].chunk_id, "C",
        "rank 2 from non-empty list should be third"
    );
    assert!(
        (result[2].score - expected_c).abs() < 1e-12,
        "C score should be 1/63, got {}",
        result[2].score
    );
}
