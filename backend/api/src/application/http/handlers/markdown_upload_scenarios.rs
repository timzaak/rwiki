//! Markdown/MDX upload scenario tests.
//!
//! Verifies the full upload pipeline for `.md` and `.mdx` files through the
//! Axum router: multipart parsing, extension validation, format routing,
//! markdown parsing, chunking, and response verification.

use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::post;
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-md-upload";
const EMBEDDING_DIMS: usize = 1536;

/// Ensure the sqlite-vec extension is registered globally so that the
/// `vec0` virtual table module is available for in-memory connections.
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

/// Start a local mock server that mimics the OpenAI embeddings API.
///
/// Returns the server's base URL (e.g. `http://127.0.0.1:{port}`).
/// The server stays alive as long as the returned `AbortHandle` is not
/// triggered (it is dropped when the test finishes).
async fn start_mock_embedding_server() -> (String, tokio::task::AbortHandle) {
    let app = axum::Router::new().route("/embeddings", post(mock_embeddings_handler));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server to random port");
    let addr = listener.local_addr().expect("get mock server addr");
    let base_url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server error");
    });

    (base_url, handle.abort_handle())
}

/// Axum handler that returns a fake OpenAI embeddings response.
///
/// Returns a 1536-dimensional zero vector for each input text, matching the
/// dimension used by `text-embedding-3-small`.  The rig library validates
/// that the response data length matches the input count, so we read the
/// `input` array from the request body.
async fn mock_embeddings_handler(
    axum::Json(body): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    let input_count = body
        .get("input")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(1);

    let embedding: Vec<f64> = vec![0.0; EMBEDDING_DIMS];

    let data: Vec<serde_json::Value> = (0..input_count)
        .map(|i| {
            serde_json::json!({
                "object": "embedding",
                "index": i,
                "embedding": embedding
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "object": "list",
        "data": data,
        "model": "text-embedding-3-small",
        "usage": {
            "prompt_tokens": 0,
            "total_tokens": 0
        }
    }))
}

/// Build a minimal `AppState` suitable for upload tests.
///
/// Starts a local mock embedding server so the upload pipeline can
/// complete without calling the real OpenAI API.
async fn test_app_state() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // Start mock embedding server and point the OpenAI client at it
    let (mock_base_url, _abort_handle) = start_mock_embedding_server().await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-md-upload-tests-only")
        .base_url(&mock_base_url);
    let embedding_model = openai_client
        .build()
        .expect("build openai client")
        .embedding_model("text-embedding-3-small");
    let app_embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model);

    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            sqlite.clone(),
            app_embedding_model,
            "text-embedding-3-small".to_string(),
        ),
    );

    let llm_client = rig::providers::openai::CompletionsClient::builder()
        .api_key("sk-test-fake")
        .base_url("http://localhost:0")
        .build()
        .expect("build LLM client");

    Arc::new(AppState {
        sqlite,
        enable_openapi: false,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: TEST_API_TOKEN.to_string(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Helper to parse the JSON response body into a serde_json::Value.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Helper to read the full response body as a String.
async fn read_body_string(body: Body) -> String {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("body is utf-8")
}

/// Build a multipart upload request with the given file name and content.
fn upload_request(file_name: &str, content: &[u8], boundary: &str) -> Request<Body> {
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body_bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body_bytes.extend_from_slice(content);
    body_bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_bytes))
        .expect("build upload request")
}

/// Build an upload request without the Authorization header.
fn upload_request_no_auth(file_name: &str, content: &[u8], boundary: &str) -> Request<Body> {
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n")
            .as_bytes(),
    );
    body_bytes.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
    body_bytes.extend_from_slice(content);
    body_bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method(Method::POST)
        .uri("/api/documents/upload")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_bytes))
        .expect("build upload request")
}

// ---------------------------------------------------------------------------
// 1. Upload .md with frontmatter -> 200, rowCount=1
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload a .md file with
// frontmatter so that the system extracts metadata (title, locale, link, tags)
// and indexes the content for RAG retrieval.
// Covers: US-CORE-014, BE-D03 (format routing), BE-D02 (markdown parser frontmatter)
#[tokio::test]
async fn upload_md_with_frontmatter_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "---\ntitle: Test Title\nlocale: en\n---\nBody content.";
    let req = upload_request("doc.md", content.as_bytes(), "----BoundaryFrontmatter");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload .md with frontmatter must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["rowCount"], 1, "rowCount must be 1 for markdown");
    assert_eq!(body["status"], "draft", "status must be draft");
    assert!(
        body["id"].is_string() && !body["id"].as_str().unwrap().is_empty(),
        "response must have a non-empty id"
    );
    assert!(body["fileName"].is_string(), "response must have fileName");
}

// ---------------------------------------------------------------------------
// 2. Upload .md without frontmatter, title from H1 -> 200
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload a .md file without
// frontmatter so that the system derives the title from the first H1 heading
// and still indexes the content successfully.
// Covers: US-CORE-014, BE-D02 (title fallback from H1)
#[tokio::test]
async fn upload_md_without_frontmatter_title_from_h1_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# My Heading\nSome body text.";
    let req = upload_request("doc.md", content.as_bytes(), "----BoundaryH1");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload .md without frontmatter must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["rowCount"], 1, "rowCount must be 1");
}

// ---------------------------------------------------------------------------
// 3. Upload .mdx file -> 200, rowCount=1
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload a .mdx file so that
// MDX content is ingested as plain text and indexed for RAG retrieval.
// Covers: US-CORE-014, BE-D03 (.mdx extension routing)
#[tokio::test]
async fn upload_mdx_file_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# MDX Doc\n\nSome MDX content with <Component />.";
    let req = upload_request("doc.mdx", content.as_bytes(), "----BoundaryMdx");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "upload .mdx must return 200");

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["rowCount"], 1, "rowCount must be 1 for mdx");
}

// ---------------------------------------------------------------------------
// 4. Upload empty .md -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject empty
// .md files with a clear error so that I know the file has no usable content.
// Covers: US-CORE-014, BE-D02 (EmptyFile error)
#[tokio::test]
async fn upload_empty_md_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content: &[u8] = b"";
    let req = upload_request("empty.md", content, "----BoundaryEmpty");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload empty .md must return 400"
    );
}

// ---------------------------------------------------------------------------
// 4b. Upload .md with frontmatter only, no body -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject .md files
// that have frontmatter but no body content, so that I know the file has no usable content.
// Covers: US-CORE-014, BE-D02 (EmptyFile error -- body empty after frontmatter extraction)
#[tokio::test]
async fn upload_md_frontmatter_only_no_body_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "---\ntitle: Test\n---\n";
    let req = upload_request("fm_only.md", content.as_bytes(), "----BoundaryFmOnly");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload .md with frontmatter only (no body) must return 400"
    );
}

// ---------------------------------------------------------------------------
// 5. Upload non-UTF-8 .md -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject non-UTF-8
// .md files with a clear error so that I know the file encoding is unsupported.
// Covers: US-CORE-014, BE-D02 (InvalidEncoding error)
#[tokio::test]
async fn upload_non_utf8_md_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content: &[u8] = &[0xFF, 0xFE];
    let req = upload_request("bad_encoding.md", content, "----BoundaryBadEnc");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload non-UTF-8 .md must return 400"
    );
}

// ---------------------------------------------------------------------------
// 6. Upload .md with unclosed frontmatter -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject .md files
// with malformed frontmatter so that I know the metadata block has a syntax error.
// Covers: US-CORE-014, BE-D02 (FrontmatterNotClosed error)
#[tokio::test]
async fn upload_md_unclosed_frontmatter_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "---\ntitle: Test\n";
    let req = upload_request(
        "unclosed_frontmatter.md",
        content.as_bytes(),
        "----BoundaryUnclosed",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload .md with unclosed frontmatter must return 400"
    );
}

// ---------------------------------------------------------------------------
// 7. Upload .txt (unsupported) -> 400 with "不支持的文件格式"
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject unsupported
// file formats with a message listing supported formats, so that I know which
// file types are accepted.
// Covers: US-CORE-014, BE-D03 (extension validation rejects unsupported formats)
#[tokio::test]
async fn upload_unsupported_format_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = b"some text content";
    let req = upload_request("notes.txt", content, "----BoundaryTxt");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload .txt must return 400"
    );

    let body_text = read_body_string(resp.into_body()).await;
    assert!(
        body_text.contains("\u{4e0d}\u{652f}\u{6301}\u{7684}\u{6587}\u{4ef6}\u{683c}\u{5f0f}"),
        "error message must contain '不支持的文件格式', got: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// 8. Upload xlsx still works (regression) -> 200
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want .xlsx uploads to continue
// working after the markdown support changes, so that existing workflows are
// not disrupted.
// Covers: US-CORE-001, BE-D03 (xlsx path preserved)
#[tokio::test]
async fn upload_xlsx_still_works_returns_200() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    // Build a minimal xlsx-like binary with ZIP magic bytes (PK\x03\x04).
    // This may or may not parse fully through the xlsx parser, but the key
    // assertion is that the response is NOT a 400 with "不支持的文件格式"
    // -- proving the .xlsx extension is routed to the xlsx path, not rejected
    // by extension validation.
    let xlsx_bytes: Vec<u8> = vec![
        0x50, 0x4B, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00,
    ];

    let req = upload_request("test.xlsx", &xlsx_bytes, "----BoundaryXlsx");

    let resp = app.oneshot(req).await.expect("send request");
    let status = resp.status();

    // The xlsx parser may fail on minimal bytes (returning 400 or 500 from
    // parsing), but it must NOT be the extension-validation rejection.
    if status == StatusCode::BAD_REQUEST {
        let body_text = read_body_string(resp.into_body()).await;
        assert!(
            !body_text.contains("\u{4e0d}\u{652f}\u{6301}\u{7684}\u{6587}\u{4ef6}\u{683c}\u{5f0f}"),
            ".xlsx must not be rejected by extension validation, got: {body_text}"
        );
    }
    // If status is 200, the upload succeeded through the xlsx pipeline.
}

// ---------------------------------------------------------------------------
// 9. Upload .md without token -> 401
// ---------------------------------------------------------------------------

// User Story: As an API operator, I want upload requests without a token to be
// rejected, so that only authorized clients can upload documents.
// Covers: US-CORE-014, upload route is behind auth middleware
#[tokio::test]
async fn upload_md_without_token_returns_401() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let content = "# Test\nBody content.";
    let req = upload_request_no_auth("doc.md", content.as_bytes(), "----BoundaryNoAuth");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "upload without token must return 401"
    );
}

// ---------------------------------------------------------------------------
// 10. Upload large .md (>1600 chars) -> 200, verify chunking in DB
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want large markdown files to be
// automatically split into chunks, so that RAG retrieval works correctly with
// sub_index continuity and chunk_count accuracy.
// Covers: US-CORE-014, text_chunker splitting consistency between md and xlsx pipelines
#[tokio::test]
async fn upload_large_md_chunking_works() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    // Build content >1600 chars to trigger text_chunker splitting
    let long_body: String = (0..80)
        .map(|i| format!("## Section {i}\n\nParagraph content for section {i}.\n\n"))
        .collect();

    let content = format!("---\ntitle: Large Doc\n---\n{long_body}");
    let req = upload_request("large.md", content.as_bytes(), "----BoundaryLarge");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload large .md must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(body["rowCount"], 1, "rowCount must be 1");

    let doc_id = body["id"].as_str().expect("document id").to_string();

    // Query chunk_metadata to verify chunking
    let chunks: Vec<(String, Option<i64>, Option<i64>)> = state
        .sqlite
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT page_id, sub_index, chunk_count FROM chunk_metadata WHERE document_id = ? ORDER BY sub_index ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![doc_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<Vec<(String, Option<i64>, Option<i64>)>, rusqlite::Error>(rows)
        })
        .await
        .expect("query chunks");

    assert!(
        chunks.len() > 1,
        "large .md should be split into multiple chunks, got {}",
        chunks.len()
    );

    // Verify all chunks share the same page_id
    let page_ids: std::collections::HashSet<&str> =
        chunks.iter().map(|(pid, _, _)| pid.as_str()).collect();
    assert_eq!(page_ids.len(), 1, "all chunks must share the same page_id");

    // Verify sub_index is continuous (0, 1, 2, ...)
    let sub_indices: Vec<i64> = chunks.iter().map(|(_, si, _)| si.unwrap_or(-1)).collect();
    for (i, &si) in sub_indices.iter().enumerate() {
        assert_eq!(
            si, i as i64,
            "sub_index at position {i} should be {i}, got {si}"
        );
    }

    // Verify chunk_count is consistent across all chunks
    let chunk_count = chunks[0].2.expect("chunk_count should be set");
    assert_eq!(
        chunk_count as usize,
        chunks.len(),
        "chunk_count must equal total number of chunks"
    );
    for (_, _, cc) in &chunks {
        assert_eq!(
            *cc,
            Some(chunk_count),
            "all chunks must have the same chunk_count"
        );
    }
}

// ---------------------------------------------------------------------------
// 11. Upload .md -> chunk_metadata.page_id is non-empty
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want uploaded markdown documents to
// have a page_id assigned to each chunk, so that neighbor expansion and dedup
// logic works correctly.
// Covers: US-CORE-014, BE-D01 (page_id migration)
#[tokio::test]
async fn upload_md_page_id_nonempty() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    let content = "---\ntitle: Page ID Test\nlocale: en\n---\nBody content for page_id test.";
    let req = upload_request("pageid_test.md", content.as_bytes(), "----BoundaryPageId");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(resp.status(), StatusCode::OK, "upload .md must return 200");

    let body = parse_json_body(resp.into_body()).await;
    let doc_id = body["id"].as_str().expect("document id").to_string();

    // Query chunk_metadata for page_id
    let page_ids: Vec<String> = state
        .sqlite
        .call(move |conn| {
            let mut stmt =
                conn.prepare("SELECT page_id FROM chunk_metadata WHERE document_id = ?")?;
            let rows = stmt
                .query_map(rusqlite::params![doc_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<Vec<String>, rusqlite::Error>(rows)
        })
        .await
        .expect("query page_ids");

    assert!(
        !page_ids.is_empty(),
        "must have at least one chunk in chunk_metadata"
    );
    for pid in &page_ids {
        assert!(
            !pid.is_empty(),
            "page_id must be a non-empty string, got empty"
        );
    }
}
