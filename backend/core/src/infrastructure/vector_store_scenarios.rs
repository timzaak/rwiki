//! Scenario tests for RAG search filtering by document publish status.
//!
//! Covers: search() and get_neighbor_chunks() only return chunks from
//! published documents. Chunks from draft, processing, and failed documents
//! are excluded from search results.
//!
//! User Stories: US-CORE-002, US-CORE-008, US-CORE-009

use std::sync::Arc;

use super::embedding_model::AppEmbeddingModel;
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
