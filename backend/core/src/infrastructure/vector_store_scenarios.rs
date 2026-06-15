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
use super::vector_store::sanitize_fts_query;
use super::vector_store::tokenize_fallback;
use super::vector_store::RetrievalScope;
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
        .get_neighbor_chunks("page_pub", 0, 3, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_draft", 0, 1, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_proc", 0, 1, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_fail", 0, 1, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_pub", 0, 2, &RetrievalScope::Published)
        .await
        .expect("should succeed");
    assert_eq!(
        pub_results.len(),
        2,
        "published doc should return 2 neighbors"
    );

    // Draft doc returns no neighbors (filtered by status)
    let drf_results = store
        .get_neighbor_chunks("page_drf", 0, 1, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_lc", 0, 2, &RetrievalScope::Published)
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
        .get_neighbor_chunks("page_lc", 0, 2, &RetrievalScope::Published)
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

// ---------------------------------------------------------------------------
// FTS index lifecycle scenario tests
// ---------------------------------------------------------------------------

/// Insert a row into the fts_chunks FTS5 index for an existing chunk_metadata row.
/// Tokenizes the content using jieba and inserts with the chunk's rowid.
async fn insert_fts_row(store: &VectorStoreManager, chunk_id: &str, content: &str) {
    let cid = chunk_id.to_string();
    let content_owned = content.to_string();
    store
        .conn
        .call(move |conn| {
            let rowid: i64 = conn.query_row(
                "SELECT rowid FROM chunk_metadata WHERE chunk_id = ?",
                rusqlite::params![cid],
                |row| row.get(0),
            )?;
            let tokens = super::vector_store::tokenize_for_fts(&content_owned);
            conn.execute(
                "INSERT OR IGNORE INTO fts_chunks(rowid, tokens) VALUES (?, ?)",
                rusqlite::params![rowid, tokens],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert_fts_row should succeed");
}

// User Story: US-CORE-002
// Covers: fts_chunks FTS5 virtual table exists after migrations run.
//         If this table is missing, all keyword search fails silently or errors.
//         This test catches migration regressions where the FTS5 DDL is dropped.
//         Uses sqlite_master to check existence, since FTS5 external-content tables
//         cannot be queried with SELECT COUNT(*) without a MATCH clause.
#[tokio::test]
async fn fts_virtual_table_exists_after_migration() {
    let store = make_sql_only_store();

    // Check via sqlite_master that the fts_chunks virtual table exists
    let exists: bool = store
        .conn
        .call(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fts_chunks'",
                [],
                |row| row.get(0),
            )?;
            Ok::<bool, rusqlite::Error>(count > 0)
        })
        .await
        .expect("querying sqlite_master should succeed");

    assert!(
        exists,
        "fts_chunks virtual table should exist after migration"
    );
}

// User Story: US-CORE-002
// Covers: After inserting a document, chunk, and FTS index row, search_by_keyword
//         finds the chunk via BM25 ranking. This verifies the end-to-end FTS pipeline:
//         tokenize -> insert -> MATCH query -> JOIN with published status -> result.
#[tokio::test]
async fn fts_index_searchable_after_chunk_insert() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_fts_search", "published").await;
    insert_test_chunk(
        &store,
        "doc_fts_search",
        "fts_chunk_0",
        "这是一段关于内存管理的测试文本",
        "page_fts_search",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "fts_chunk_0", "这是一段关于内存管理的测试文本").await;

    let results = store
        .search_by_keyword("内存管理", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(results.len(), 1, "should find 1 result for '内存管理'");
    assert_eq!(results[0].chunk_id, "fts_chunk_0");
    assert!(
        results[0].score > 0.0,
        "BM25 score should be positive, got {}",
        results[0].score
    );
}

// User Story: US-CORE-002
// Covers: Deleting a document via remove_document removes all associated FTS entries.
//         After deletion, search_by_keyword returns empty for previously indexed content.
//         This verifies the FTS cleanup in remove_document (DELETE FROM fts_chunks).
#[tokio::test]
async fn deleting_document_removes_fts_entries() {
    let store = make_sql_only_store();

    let doc_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    insert_test_document(&store, "00000000-0000-0000-0000-000000000001", "published").await;
    insert_test_chunk(
        &store,
        "00000000-0000-0000-0000-000000000001",
        "del_chunk_0",
        "关于部署流程的详细说明文档",
        "page_del",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "del_chunk_0", "关于部署流程的详细说明文档").await;

    // Verify it is searchable before deletion
    let before = store
        .search_by_keyword("部署流程", 5, &RetrievalScope::Published)
        .await
        .expect("search should succeed");
    assert_eq!(before.len(), 1, "should find chunk before deletion");

    // Delete the document
    store
        .remove_document(&doc_id)
        .await
        .expect("remove_document should succeed");

    // Verify FTS entries are gone
    let after = store
        .search_by_keyword("部署流程", 5, &RetrievalScope::Published)
        .await
        .expect("search should succeed after deletion");
    assert!(
        after.is_empty(),
        "should find nothing after document deletion"
    );
}

// User Story: US-CORE-002
// Covers: search_by_keyword excludes chunks from draft documents.
//         Draft documents represent in-progress content that must not be surfaced
//         to users querying the knowledge base. The SQL JOIN on d.status = 'published'
//         in search_by_keyword enforces this at the query level.
#[tokio::test]
async fn fts_search_excludes_draft_documents() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_fts_draft", "draft").await;
    insert_test_chunk(
        &store,
        "doc_fts_draft",
        "draft_fts_chunk",
        "草稿文档中的网络协议分析内容",
        "page_fts_draft",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "draft_fts_chunk", "草稿文档中的网络协议分析内容").await;

    let results = store
        .search_by_keyword("网络协议", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert!(
        results.is_empty(),
        "draft document chunks should not appear in keyword search, got {} results",
        results.len()
    );
}

// User Story: US-CORE-002
// Covers: search_by_keyword includes chunks from published documents.
//         This is the baseline positive case -- published content must be findable
//         via keyword search. Without this guarantee, the knowledge base is useless.
#[tokio::test]
async fn fts_search_includes_published_documents() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_fts_pub", "published").await;
    insert_test_chunk(
        &store,
        "doc_fts_pub",
        "pub_fts_chunk",
        "已发布文档中的数据库优化策略",
        "page_fts_pub",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "pub_fts_chunk", "已发布文档中的数据库优化策略").await;

    let results = store
        .search_by_keyword("数据库优化", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(results.len(), 1, "published document should be found");
    assert_eq!(results[0].chunk_id, "pub_fts_chunk");
}

// Regression: search_by_keyword under RetrievalScope::Collection must bind the
// collection ids to the `cm.document_id IN (...)` placeholders and top_k to the
// trailing `LIMIT ?`. A prior version pushed top_k BEFORE the ids, so the id
// landed on LIMIT (coerced to 0) and top_k landed inside IN(...) — silently
// returning empty for every scoped keyword query, which broke US-CORE-036
// (eval on a draft batch runs hybrid = keyword + vector).
#[tokio::test]
async fn fts_search_collection_scope_returns_draft_chunks() {
    let store = make_sql_only_store();

    // Draft document — invisible under the default Published scope
    insert_test_document(&store, "doc_fts_coll", "draft").await;
    insert_test_chunk(
        &store,
        "doc_fts_coll",
        "coll_fts_chunk",
        "草稿批次的容器编排与弹性伸缩方案",
        "page_fts_coll",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "coll_fts_chunk", "草稿批次的容器编排与弹性伸缩方案").await;

    // Published scope must exclude the draft
    let published = store
        .search_by_keyword("容器编排", 5, &RetrievalScope::Published)
        .await
        .expect("published search should succeed");
    assert!(
        published.is_empty(),
        "draft must not be visible under Published scope"
    );

    // Collection scope must surface the draft chunk — the eval-on-draft path
    let scoped = store
        .search_by_keyword(
            "容器编排",
            5,
            &RetrievalScope::Collection(vec!["doc_fts_coll".to_string()]),
        )
        .await
        .expect("collection search should succeed");
    assert_eq!(
        scoped.len(),
        1,
        "collection scope must return the draft chunk (param-order regression)"
    );
    assert_eq!(scoped[0].chunk_id, "coll_fts_chunk");
}

// User Story: US-CORE-002
// Covers: When both published and draft documents have chunks matching a keyword query,
//         only the published document's chunks are returned. This is the critical
//         access control guarantee: draft content never leaks to search users,
//         even when the FTS index contains tokens from both documents.
#[tokio::test]
async fn fts_search_filters_mixed_status_returns_only_published() {
    let store = make_sql_only_store();

    // Published document
    insert_test_document(&store, "doc_fts_mixed_pub", "published").await;
    insert_test_chunk(
        &store,
        "doc_fts_mixed_pub",
        "mixed_pub_chunk",
        "容器编排与微服务部署最佳实践",
        "page_mixed_pub",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "mixed_pub_chunk", "容器编排与微服务部署最佳实践").await;

    // Draft document with overlapping content
    insert_test_document(&store, "doc_fts_mixed_drf", "draft").await;
    insert_test_chunk(
        &store,
        "doc_fts_mixed_drf",
        "mixed_draft_chunk",
        "容器编排的草稿版本微服务部署计划",
        "page_mixed_drf",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(
        &store,
        "mixed_draft_chunk",
        "容器编排的草稿版本微服务部署计划",
    )
    .await;

    let results = store
        .search_by_keyword("容器编排", 10, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(
        results.len(),
        1,
        "only published document should be returned"
    );
    assert_eq!(
        results[0].chunk_id, "mixed_pub_chunk",
        "only the published chunk should appear"
    );
    assert!(
        !results.iter().any(|r| r.chunk_id == "mixed_draft_chunk"),
        "draft chunk must not appear in results"
    );
}

// User Story: US-CORE-002
// Covers: Full FTS lifecycle -- write (insert doc+chunk+fts), search (find it),
//         delete (remove_document), search again (empty). This end-to-end test
//         verifies that all three phases interact correctly: indexing populates FTS,
//         search reads FTS with status filter, and deletion cleans up FTS entries.
//         Any break in this chain causes the final assertion to fail.
#[tokio::test]
async fn fts_lifecycle_write_search_delete_search_empty() {
    let store = make_sql_only_store();

    let doc_id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();

    // Phase 1: Write -- insert document, chunk, and FTS index
    insert_test_document(&store, "00000000-0000-0000-0000-000000000002", "published").await;
    insert_test_chunk(
        &store,
        "00000000-0000-0000-0000-000000000002",
        "lifecycle_chunk",
        "全文检索生命周期的集成测试内容",
        "page_lifecycle",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "lifecycle_chunk", "全文检索生命周期的集成测试内容").await;

    // Phase 2: Search -- verify content is findable
    let search_after_write = store
        .search_by_keyword("全文检索", 5, &RetrievalScope::Published)
        .await
        .expect("search after write should succeed");
    assert_eq!(
        search_after_write.len(),
        1,
        "should find 1 result after writing"
    );
    assert_eq!(search_after_write[0].chunk_id, "lifecycle_chunk");

    // Phase 3: Delete -- remove the document
    store
        .remove_document(&doc_id)
        .await
        .expect("remove_document should succeed");

    // Phase 4: Search again -- should return empty
    let search_after_delete = store
        .search_by_keyword("全文检索", 5, &RetrievalScope::Published)
        .await
        .expect("search after delete should succeed");
    assert!(
        search_after_delete.is_empty(),
        "should find nothing after deleting the document, got {} results",
        search_after_delete.len()
    );
}

// ---------------------------------------------------------------------------
// Keyword search tokenization and sanitization scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: Chinese keyword exact match via FTS. After inserting a document, chunk,
//         and FTS row with Chinese content "Rust 语言的内存管理", searching for the
//         single token "内存" must find the chunk. This validates that jieba tokenization
//         splits Chinese text into individual tokens and FTS5 BM25 matches on them.
#[tokio::test]
async fn chinese_keyword_exact_match_via_fts() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_cn_exact", "published").await;
    insert_test_chunk(
        &store,
        "doc_cn_exact",
        "cn_exact_chunk",
        "Rust 语言的内存管理",
        "page_cn_exact",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "cn_exact_chunk", "Rust 语言的内存管理").await;

    let results = store
        .search_by_keyword("内存", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(
        results.len(),
        1,
        "should find 1 result for Chinese token '内存'"
    );
    assert_eq!(results[0].chunk_id, "cn_exact_chunk");
}

// User Story: US-CORE-002
// Covers: English keyword exact match via FTS. After inserting a document, chunk,
//         and FTS row with English content "Kubernetes deployment guide", searching for
//         "Kubernetes" must find the chunk. This validates that English words are
//         tokenized as whole tokens and FTS5 can match them.
#[tokio::test]
async fn english_keyword_exact_match_via_fts() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_en_exact", "published").await;
    insert_test_chunk(
        &store,
        "doc_en_exact",
        "en_exact_chunk",
        "Kubernetes deployment guide",
        "page_en_exact",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "en_exact_chunk", "Kubernetes deployment guide").await;

    let results = store
        .search_by_keyword("Kubernetes", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(
        results.len(),
        1,
        "should find 1 result for English token 'Kubernetes'"
    );
    assert_eq!(results[0].chunk_id, "en_exact_chunk");
}

// User Story: US-CORE-002
// Covers: Mixed Chinese-English content is searchable by both Chinese and English tokens.
//         Content "Rust 语言的内存管理" must be findable by both "Rust" and "内存".
//         This validates that jieba tokenization correctly separates Chinese and English
//         segments, and that each produces independent searchable tokens in the FTS index.
#[tokio::test]
async fn mixed_chinese_english_tokenization_searchable() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_mixed", "published").await;
    insert_test_chunk(
        &store,
        "doc_mixed",
        "mixed_chunk",
        "Rust 语言的内存管理",
        "page_mixed",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "mixed_chunk", "Rust 语言的内存管理").await;

    // Search by English token
    let en_results = store
        .search_by_keyword("Rust", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword for 'Rust' should succeed");
    assert_eq!(
        en_results.len(),
        1,
        "should find chunk via English token 'Rust'"
    );
    assert_eq!(en_results[0].chunk_id, "mixed_chunk");

    // Search by Chinese token
    let cn_results = store
        .search_by_keyword("内存", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword for '内存' should succeed");
    assert_eq!(
        cn_results.len(),
        1,
        "should find chunk via Chinese token '内存'"
    );
    assert_eq!(cn_results[0].chunk_id, "mixed_chunk");
}

// User Story: US-CORE-002
// Covers: sanitize_fts_query strips FTS5 operators (AND, OR, NOT) and special characters
//         (quotes, asterisks, parentheses) to prevent injection or syntax errors in
//         the MATCH clause. If these were not stripped, a user query containing them
//         could break the FTS5 query or change its semantics.
#[test]
fn sanitize_fts_query_strips_operators_and_special_chars() {
    // FTS5 operators should be removed
    assert_eq!(
        sanitize_fts_query("memory AND management"),
        "memory management",
        "AND operator should be stripped"
    );
    assert_eq!(
        sanitize_fts_query("memory OR cpu"),
        "memory cpu",
        "OR operator should be stripped"
    );
    assert_eq!(
        sanitize_fts_query("memory NOT leak"),
        "memory leak",
        "NOT operator should be stripped"
    );

    // Special characters should be removed
    assert_eq!(
        sanitize_fts_query("\"quoted\""),
        "quoted",
        "double quotes should be stripped"
    );
    assert_eq!(
        sanitize_fts_query("wild*card"),
        "wildcard",
        "asterisks should be stripped"
    );
    assert_eq!(
        sanitize_fts_query("(grouped)"),
        "grouped",
        "parentheses should be stripped"
    );

    // Combined operators and special chars
    assert_eq!(
        sanitize_fts_query("test\" AND (OR) *end"),
        "test end",
        "all operators and special chars should be stripped together"
    );
}

// User Story: US-CORE-002
// Covers: sanitize_fts_query preserves valid Chinese and mixed tokens after sanitization.
//         Chinese characters, English words, and mixed tokens must pass through unchanged.
//         If the sanitizer were too aggressive (e.g. stripping non-ASCII), Chinese keyword
//         search would break entirely.
#[test]
fn sanitize_fts_query_preserves_valid_tokens() {
    // Pure Chinese tokens
    assert_eq!(
        sanitize_fts_query("内存 管理"),
        "内存 管理",
        "Chinese tokens should be preserved"
    );

    // Mixed Chinese and English
    assert_eq!(
        sanitize_fts_query("Rust 语言"),
        "Rust 语言",
        "mixed Chinese-English tokens should be preserved"
    );

    // Single Chinese token
    assert_eq!(
        sanitize_fts_query("部署"),
        "部署",
        "single Chinese token should be preserved"
    );
}

// User Story: US-CORE-002
// Covers: sanitize_fts_query handles edge cases: empty string, whitespace-only, and
//         input that becomes empty after stripping operators. These cases must return
//         an empty string so that search_by_keyword can short-circuit to an empty result
//         rather than passing an invalid MATCH clause to FTS5.
#[test]
fn sanitize_fts_query_handles_empty_and_whitespace() {
    assert_eq!(
        sanitize_fts_query(""),
        "",
        "empty input should return empty string"
    );
    assert_eq!(
        sanitize_fts_query("   "),
        "",
        "whitespace-only input should return empty string"
    );
    assert_eq!(
        sanitize_fts_query("AND OR NOT"),
        "",
        "operators-only input should return empty string"
    );
    assert_eq!(
        sanitize_fts_query("  AND  OR  "),
        "",
        "whitespace with operators should return empty string"
    );
}

// User Story: US-CORE-002
// Covers: search_by_keyword returns Ok(empty) when no FTS content matches the query.
//         This must not be an error -- it is a normal "no results" case. If this returned
//         Err, callers would need to distinguish "no match" from "system error", adding
//         unnecessary complexity.
#[tokio::test]
async fn fts_search_no_match_returns_empty_not_error() {
    let store = make_sql_only_store();

    // No documents or chunks inserted at all
    let results = store
        .search_by_keyword("完全不相关的查询内容", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should return Ok even with no matches");

    assert!(
        results.is_empty(),
        "no-match query should return empty Vec, got {} results",
        results.len()
    );
}

// User Story: US-CORE-002
// Covers: tokenize_fallback produces non-empty output for edge cases where jieba
//         might fail (special encoding, unusual characters). It splits on non-alphanumeric
//         characters, so it must always produce at least one token for alphanumeric input
//         and handle empty/whitespace gracefully. This is the safety net for tokenization.
#[test]
fn tokenization_fallback_produces_output_for_edge_cases() {
    // Normal input
    let result = tokenize_fallback("hello world");
    assert!(
        !result.is_empty(),
        "should produce tokens for 'hello world'"
    );
    assert!(result.contains("hello"), "should contain 'hello'");
    assert!(result.contains("world"), "should contain 'world'");

    // Mixed alphanumeric and special chars
    let result = tokenize_fallback("key-value_pair");
    assert!(
        result.contains("key") && result.contains("value") && result.contains("pair"),
        "should split on hyphens and underscores, got: {result}"
    );

    // Empty input
    let result = tokenize_fallback("");
    assert!(result.is_empty(), "empty input should produce empty output");

    // Whitespace-only input
    let result = tokenize_fallback("   ");
    assert!(
        result.is_empty(),
        "whitespace-only should produce empty output"
    );

    // Non-alphanumeric only (e.g. all symbols)
    let result = tokenize_fallback("---***");
    assert!(
        result.is_empty(),
        "non-alphanumeric only should produce empty output, got: {result}"
    );
}

// ---------------------------------------------------------------------------
// Hybrid search RRF fusion scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-019
// Covers: When FTS and vector result lists share an overlapping chunk, rrf_fuse must
//         deduplicate by chunk_id and assign the combined RRF score. Chunk B appearing
//         in both lists should appear once with accumulated score, ranked higher than
//         chunks from only one list. This is the core dedup guarantee for hybrid search.
#[test]
fn hybrid_search_rrf_fusion_no_duplicate_chunks() {
    let fts_results = vec![
        make_rrf_search_result("A", "fts content A", 0.9, "p1"),
        make_rrf_search_result("B", "shared content B", 0.8, "p1"),
        make_rrf_search_result("C", "fts content C", 0.7, "p2"),
    ];
    let vec_results = vec![
        make_rrf_search_result("B", "shared content B", 0.95, "p1"),
        make_rrf_search_result("D", "vec content D", 0.85, "p3"),
    ];

    let fused = rrf_fuse(&[fts_results, vec_results], 60, 10);

    // Verify no duplicate chunk_ids
    let ids: Vec<&str> = fused.iter().map(|r| r.chunk_id.as_str()).collect();
    let unique_ids: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(
        ids.len(),
        unique_ids.len(),
        "fused results must not contain duplicate chunk_ids, got {:?}",
        ids
    );

    // Should have exactly 4 unique chunks
    assert_eq!(fused.len(), 4, "should have 4 unique chunks (A, B, C, D)");

    // B should rank first because it appears in both lists
    assert_eq!(
        fused[0].chunk_id, "B",
        "B should rank first (appears in both FTS and vector)"
    );

    // B's combined score: rank 1 in fts_results + rank 0 in vec_results
    // = 1/(60+1+1) + 1/(60+0+1) = 1/62 + 1/61
    let expected_b = 1.0 / 62.0 + 1.0 / 61.0;
    assert!(
        (fused[0].score - expected_b).abs() < 1e-12,
        "B's combined RRF score should be 1/62 + 1/61 = {}, got {}",
        expected_b,
        fused[0].score
    );
}

// User Story: US-CORE-019
// Covers: When FTS returns nothing (e.g. query has no FTS tokens or no FTS index exists)
//         but vector search produces results, rrf_fuse should return the vector results
//         with correct single-list RRF scores. This models the degradation path where
//         keyword search contributes nothing and the system falls back to vector-only.
#[test]
fn hybrid_search_with_empty_fts_returns_vector_results() {
    let fts_results: Vec<SearchResult> = vec![];
    let vec_results = vec![
        make_rrf_search_result("V1", "vector content 1", 0.9, "p1"),
        make_rrf_search_result("V2", "vector content 2", 0.8, "p2"),
    ];

    let fused = rrf_fuse(&[fts_results, vec_results], 60, 10);

    assert_eq!(fused.len(), 2, "should return 2 vector results");
    assert_eq!(fused[0].chunk_id, "V1");
    assert_eq!(fused[1].chunk_id, "V2");

    // Verify single-list RRF scores
    let expected_v1 = 1.0 / (60.0 + 0.0 + 1.0); // rank 0: 1/61
    let expected_v2 = 1.0 / (60.0 + 1.0 + 1.0); // rank 1: 1/62
    assert!(
        (fused[0].score - expected_v1).abs() < 1e-12,
        "V1 RRF score should be 1/61, got {}",
        fused[0].score
    );
    assert!(
        (fused[1].score - expected_v2).abs() < 1e-12,
        "V2 RRF score should be 1/62, got {}",
        fused[1].score
    );
}

// User Story: US-CORE-019
// Covers: When vector search returns nothing (e.g. no embeddings indexed) but FTS
//         produces results, rrf_fuse should return the FTS results with correct
//         single-list RRF scores. This models the degradation path where semantic
//         search contributes nothing and keyword search carries the query.
#[test]
fn hybrid_search_with_empty_vector_returns_fts_results() {
    let fts_results = vec![
        make_rrf_search_result("F1", "keyword content 1", 0.9, "p1"),
        make_rrf_search_result("F2", "keyword content 2", 0.8, "p2"),
    ];
    let vec_results: Vec<SearchResult> = vec![];

    let fused = rrf_fuse(&[fts_results, vec_results], 60, 10);

    assert_eq!(fused.len(), 2, "should return 2 FTS results");
    assert_eq!(fused[0].chunk_id, "F1");
    assert_eq!(fused[1].chunk_id, "F2");

    // Verify single-list RRF scores
    let expected_f1 = 1.0 / (60.0 + 0.0 + 1.0); // rank 0: 1/61
    let expected_f2 = 1.0 / (60.0 + 1.0 + 1.0); // rank 1: 1/62
    assert!(
        (fused[0].score - expected_f1).abs() < 1e-12,
        "F1 RRF score should be 1/61, got {}",
        fused[0].score
    );
    assert!(
        (fused[1].score - expected_f2).abs() < 1e-12,
        "F2 RRF score should be 1/62, got {}",
        fused[1].score
    );
}

// User Story: US-CORE-002
// Covers: backfill_fts_index populates fts_chunks for existing chunk_metadata rows
//         that were inserted via raw SQL (bypassing FTS sync), simulating a pre-M3
//         database state. After backfill, search_by_keyword must find the content.
//         This validates the startup migration path for upgrading existing databases.
#[tokio::test]
async fn startup_backfill_populates_fts_index_from_existing_chunks() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_backfill", "published").await;

    // Insert chunks via raw SQL directly into chunk_metadata only (no FTS row),
    // simulating pre-FTS state where chunks exist but have no FTS index entries.
    let ndims = store.ndims();
    let doc_id = "doc_backfill".to_string();
    let cid = "bf_chunk_0".to_string();
    let content = "回填测试内容关于负载均衡策略".to_string();
    let pid = "page_bf".to_string();
    let ndims_copy = ndims;
    store
        .conn
        .call(move |conn| {
            let dummy_embedding = vec![0u8; ndims_copy * 4];
            conn.execute(
                "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) \
                 VALUES (?, ?, ?, '', NULL, NULL, '', NULL, ?, 0, 1, NULL, NULL)",
                rusqlite::params![doc_id, cid, content, pid],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
                rusqlite::params![rowid, dummy_embedding],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("raw SQL insert should succeed");

    // Verify FTS is empty before backfill -- search should return nothing
    let results_before = store
        .search_by_keyword("负载均衡", 5, &RetrievalScope::Published)
        .await
        .expect("search before backfill should succeed");
    assert!(
        results_before.is_empty(),
        "keyword search should return nothing before backfill"
    );

    // Run backfill
    store
        .backfill_fts_index()
        .await
        .expect("backfill_fts_index should succeed");

    // Verify the chunk is now findable via keyword search
    let results = store
        .search_by_keyword("负载均衡", 5, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed after backfill");

    assert_eq!(
        results.len(),
        1,
        "backfill should make chunk findable via keyword search"
    );
    assert_eq!(results[0].chunk_id, "bf_chunk_0");
}

// User Story: US-CORE-002
// Covers: backfill_fts_index is idempotent -- running it multiple times must not create
//         duplicate FTS entries. This is critical because backfill runs on every startup;
//         if it were not idempotent, each restart would accumulate duplicate index rows,
//         inflating BM25 scores and corrupting search ranking.
#[tokio::test]
async fn backfill_idempotent_multiple_runs_no_duplicates() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_idempotent", "published").await;
    insert_test_chunk(
        &store,
        "doc_idempotent",
        "idem_chunk",
        "幂等性测试关于并发控制机制",
        "page_idem",
        Some(0),
        Some(1),
    )
    .await;

    // No FTS row yet -- backfill will create it
    store
        .backfill_fts_index()
        .await
        .expect("first backfill should succeed");

    let results_after_first = store
        .search_by_keyword("并发控制", 5, &RetrievalScope::Published)
        .await
        .expect("search after first backfill should succeed");
    assert_eq!(
        results_after_first.len(),
        1,
        "should find 1 result after first backfill"
    );

    // Run backfill again
    store
        .backfill_fts_index()
        .await
        .expect("second backfill should succeed");

    // Verify no duplicate results -- still exactly 1, not 2
    let results_after_second = store
        .search_by_keyword("并发控制", 5, &RetrievalScope::Published)
        .await
        .expect("search after second backfill should succeed");
    assert_eq!(
        results_after_second.len(),
        1,
        "second backfill should not create duplicates, still 1 result"
    );
    assert_eq!(results_after_second[0].chunk_id, "idem_chunk");
}

// User Story: US-CORE-002
// Covers: If the fts_chunks table is dropped or corrupted, search_by_keyword must degrade
//         gracefully by returning Ok(empty) rather than Err. This is the resilience
//         guarantee for hybrid search: FTS failure does not crash the system, it degrades
//         to vector-only. The design in search_hybrid wraps FTS in a match with a warning
//         log, and search_by_keyword itself should also handle the missing table case.
//         NOTE: If the current search_by_keyword returns Err when fts_chunks is missing,
//         this test will fail at runtime. That outcome is acceptable for the authoring phase;
//         the runner phase will verify actual behavior and adjust if needed.
#[tokio::test]
async fn fts_failure_degrades_gracefully_not_error() {
    let store = make_sql_only_store();

    insert_test_document(&store, "doc_fts_fail", "published").await;
    insert_test_chunk(
        &store,
        "doc_fts_fail",
        "fts_fail_chunk",
        "容错测试关于缓存穿透防护",
        "page_fts_fail",
        Some(0),
        Some(1),
    )
    .await;
    insert_fts_row(&store, "fts_fail_chunk", "容错测试关于缓存穿透防护").await;

    // Drop the fts_chunks table to simulate FTS corruption/absence
    store
        .conn
        .call(move |conn| {
            conn.execute_batch("DROP TABLE IF EXISTS fts_chunks;")?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("dropping fts_chunks should succeed");

    // search_by_keyword should return Ok(empty) rather than Err
    let result = store
        .search_by_keyword("缓存穿透", 5, &RetrievalScope::Published)
        .await;

    match result {
        Ok(results) => {
            assert!(
                results.is_empty(),
                "FTS failure should degrade to empty results, got {} results",
                results.len()
            );
        }
        Err(e) => {
            // If search_by_keyword currently returns Err when fts_chunks is missing,
            // this test documents the behavior gap. The runner phase will determine
            // if this needs a production code fix to return Ok(empty) instead.
            panic!("search_by_keyword returned Err on missing fts_chunks, expected Ok(empty): {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Self-contained deduplication scenario tests
// ---------------------------------------------------------------------------

// User Story: batch-refresh
// Covers: Self-contained deduplication - two documents with identical content
//         both have independent chunk_metadata entries. When the old document goes
//         offline and the new document goes online, the new document's chunks are
//         still retrievable. This validates the core benefit of self-contained
//         deduplication over the old skip-based approach.
#[tokio::test]
async fn old_offline_new_still_retrievable() {
    let store = make_sql_only_store();

    // Given: 新老批次有相同内容
    let old_doc_id = "doc_old_batch".to_string();
    let new_doc_id = "doc_new_batch".to_string();

    // Insert old document as published with chunks
    insert_test_document(&store, &old_doc_id, "published").await;
    insert_test_chunk(
        &store,
        &old_doc_id,
        "chunk_shared_old",
        "shared content across old and new batches",
        "page_shared",
        Some(0),
        Some(1),
    )
    .await;

    // Insert new document with same content (as draft)
    insert_test_document(&store, &new_doc_id, "draft").await;
    insert_test_chunk(
        &store,
        &new_doc_id,
        "chunk_shared_new",
        "shared content across old and new batches",
        "page_shared",
        Some(0),
        Some(1),
    )
    .await;

    // Verify both documents have their own independent chunk entries
    let old_doc_id_for_query = old_doc_id.clone();
    let old_doc_chunks = store
        .conn
        .call(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM chunk_metadata WHERE document_id = ?",
                rusqlite::params![old_doc_id_for_query],
                |row| row.get(0),
            )?;
            Ok::<i64, rusqlite::Error>(count)
        })
        .await
        .expect("query should succeed");

    let new_doc_id_for_query = new_doc_id.clone();
    let new_doc_chunks = store
        .conn
        .call(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM chunk_metadata WHERE document_id = ?",
                rusqlite::params![new_doc_id_for_query],
                |row| row.get(0),
            )?;
            Ok::<i64, rusqlite::Error>(count)
        })
        .await
        .expect("query should succeed");

    assert_eq!(old_doc_chunks, 1, "老文档应有 1 条 chunk_metadata");
    assert_eq!(
        new_doc_chunks, 1,
        "新文档应有 1 条 chunk_metadata（自包含）"
    );

    // When: 老文档下线（状态改为 draft）
    let old_doc_id_for_update = old_doc_id.clone();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'draft' WHERE id = ?",
                rusqlite::params![old_doc_id_for_update],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("update old doc status should succeed");

    // And: 新文档上线（状态改为 published）
    let new_doc_id_for_update = new_doc_id.clone();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ?",
                rusqlite::params![new_doc_id_for_update],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("update new doc status should succeed");

    // Then: 新文档仍能检索到相同内容（通过 get_neighbor_chunks）
    let results = store
        .get_neighbor_chunks("page_shared", 0, 5, &RetrievalScope::Published)
        .await
        .expect("get_neighbor_chunks should succeed");

    assert_eq!(results.len(), 1, "应能检索到新文档的 chunk");
    assert_eq!(
        results[0].chunk_id, "chunk_shared_new",
        "应返回新文档的 chunk ID"
    );
}

// User Story: US-CORE-002
// Covers: backfill indexes all chunks regardless of document status, but search_by_keyword
//         only returns chunks from published documents. This separation of concerns is
//         critical: backfill must not skip drafts (they may be published later), and
//         search must never surface drafts. After updating a draft to published, the
//         previously invisible chunks must appear in search results.
#[tokio::test]
async fn backfill_indexes_all_chunks_but_search_filters_by_status() {
    let store = make_sql_only_store();

    // Insert a published document with a chunk (via raw SQL, no FTS sync)
    let ndims = store.ndims();
    let pub_doc_id = "doc_bf_pub".to_string();
    let pub_cid = "bf_pub_chunk".to_string();
    let pub_content = "已发布文档的API网关配置说明".to_string();
    let pub_pid = "page_bf_pub".to_string();
    let ndims_pub = ndims;
    store
        .conn
        .call(move |conn| {
            let dummy_embedding = vec![0u8; ndims_pub * 4];
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count) VALUES (?, 'pub.xlsx', 'published', 1)",
                rusqlite::params![pub_doc_id],
            )?;
            conn.execute(
                "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) \
                 VALUES (?, ?, ?, '', NULL, NULL, '', NULL, ?, 0, 1, NULL, NULL)",
                rusqlite::params![pub_doc_id, pub_cid, pub_content, pub_pid],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
                rusqlite::params![rowid, dummy_embedding],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert published doc+chunk should succeed");

    // Insert a draft document with a chunk (via raw SQL, no FTS sync)
    let draft_doc_id = "doc_bf_draft".to_string();
    let draft_cid = "bf_draft_chunk".to_string();
    let draft_content = "草稿文档的API网关设计计划".to_string();
    let draft_pid = "page_bf_draft".to_string();
    let ndims_draft = ndims;
    store
        .conn
        .call(move |conn| {
            let dummy_embedding = vec![0u8; ndims_draft * 4];
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count) VALUES (?, 'draft.xlsx', 'draft', 1)",
                rusqlite::params![draft_doc_id],
            )?;
            conn.execute(
                "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) \
                 VALUES (?, ?, ?, '', NULL, NULL, '', NULL, ?, 0, 1, NULL, NULL)",
                rusqlite::params![draft_doc_id, draft_cid, draft_content, draft_pid],
            )?;
            let rowid = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
                rusqlite::params![rowid, dummy_embedding],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("insert draft doc+chunk should succeed");

    // Run backfill -- indexes all chunks regardless of status
    store
        .backfill_fts_index()
        .await
        .expect("backfill should succeed");

    // Search should only return the published document's chunk
    let results = store
        .search_by_keyword("API网关", 10, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed");

    assert_eq!(
        results.len(),
        1,
        "should find only 1 result (published), got {}",
        results.len()
    );
    assert_eq!(
        results[0].chunk_id, "bf_pub_chunk",
        "only published chunk should appear"
    );
    assert!(
        !results.iter().any(|r| r.chunk_id == "bf_draft_chunk"),
        "draft chunk must not appear in search results"
    );

    // Verify FTS index has both chunks (backfill does not filter by status).
    // We cannot SELECT COUNT(*) from an FTS5 external-content table without MATCH,
    // so instead we temporarily publish the draft doc and verify both chunks appear.
    let doc_id_to_temp_publish = "doc_bf_draft".to_string();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ?",
                rusqlite::params![doc_id_to_temp_publish],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("temp publish should succeed");

    let all_results = store
        .search_by_keyword("API网关", 10, &RetrievalScope::Published)
        .await
        .expect("search for all chunks should succeed");
    assert_eq!(
        all_results.len(),
        2,
        "fts_chunks should have both chunks indexed (backfill does not filter by status), got {}",
        all_results.len()
    );

    // Revert the draft status for the rest of the test
    let doc_id_to_revert = "doc_bf_draft".to_string();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'draft' WHERE id = ?",
                rusqlite::params![doc_id_to_revert],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("revert draft should succeed");

    // Now publish the draft document
    let doc_id_to_publish = "doc_bf_draft".to_string();
    store
        .conn
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ?",
                rusqlite::params![doc_id_to_publish],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("update status should succeed");

    // Search again -- now both chunks should be returned
    let results_after_publish = store
        .search_by_keyword("API网关", 10, &RetrievalScope::Published)
        .await
        .expect("search_by_keyword should succeed after publishing");

    assert_eq!(
        results_after_publish.len(),
        2,
        "both published chunks should be found after status update, got {}",
        results_after_publish.len()
    );
    let found_ids: Vec<&str> = results_after_publish
        .iter()
        .map(|r| r.chunk_id.as_str())
        .collect();
    assert!(
        found_ids.contains(&"bf_pub_chunk"),
        "published chunk should still be found"
    );
    assert!(
        found_ids.contains(&"bf_draft_chunk"),
        "newly published chunk should now be found"
    );
}
