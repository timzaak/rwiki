//! OpenAPI JSON upload scenario tests.
//!
//! Verifies the full upload pipeline for OpenAPI 3.x JSON files through the
//! Axum router: multipart parsing, JSON validation, endpoint extraction,
//! embedding, and the upload -> publish -> list workflow.

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

const TEST_API_TOKEN: &str = "test-openapi-upload-token";
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
/// dimension used by `text-embedding-3-small`.
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

/// Build a minimal `AppState` suitable for OpenAPI upload tests.
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

    let (mock_base_url, _abort_handle) = start_mock_embedding_server().await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-openapi-upload-tests-only")
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

/// Build an authorized request for a given method and URI.
fn auth_request(method: Method, uri: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {TEST_API_TOKEN}"))
        .body(Body::empty())
        .expect("build request")
}

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

/// Valid minimal OpenAPI 3.0 JSON with 2 endpoints (GET /pets, POST /pets).
fn valid_openapi_json() -> Vec<u8> {
    r#"{
  "openapi": "3.0.0",
  "info": { "title": "Petstore", "version": "1.0.0" },
  "paths": {
    "/pets": {
      "get": {
        "operationId": "listPets",
        "summary": "List all pets",
        "responses": { "200": { "description": "A list of pets" } }
      },
      "post": {
        "operationId": "createPet",
        "summary": "Create a pet",
        "responses": { "201": { "description": "Pet created" } }
      }
    }
  }
}"#
    .as_bytes()
    .to_vec()
}

/// Plain JSON that is NOT a valid OpenAPI document (missing `openapi` field).
fn non_openapi_json() -> Vec<u8> {
    r#"{"hello": "world"}"#.as_bytes().to_vec()
}

/// Valid OpenAPI 3.0 JSON with empty paths object.
fn openapi_empty_paths_json() -> Vec<u8> {
    r#"{
  "openapi": "3.0.0",
  "info": { "title": "Empty", "version": "1.0.0" },
  "paths": {}
}"#
    .as_bytes()
    .to_vec()
}

// ---------------------------------------------------------------------------
// 1. Upload valid OpenAPI JSON -> 200, status=draft, rowCount=2
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload a valid OpenAPI 3.x
// JSON file so that each API endpoint becomes an independent knowledge page in
// draft status, ready for review before publishing.
// Covers: US-CORE-018, BE-D01 (openapi parser + .json routing)

#[tokio::test]
async fn upload_valid_openapi_json_returns_200_draft() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request(
        "petstore.json",
        &valid_openapi_json(),
        "----BoundaryOpenApiValid",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "upload valid OpenAPI JSON must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["status"], "draft",
        "uploaded document must have 'draft' status"
    );
    assert_eq!(body["rowCount"], 2, "rowCount must be 2 for 2 endpoints");
    assert!(
        body["id"].is_string() && !body["id"].as_str().unwrap().is_empty(),
        "response must have a non-empty id"
    );
    assert_eq!(
        body["fileName"], "petstore.json",
        "fileName must be petstore.json"
    );
}

// ---------------------------------------------------------------------------
// 2. Upload non-OpenAPI JSON -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject plain
// JSON files that are not OpenAPI format with a clear error, so that I know
// the file lacks the required `openapi` field.
// Covers: US-CORE-018, BE-D01 (format validation)

#[tokio::test]
async fn upload_non_openapi_json_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request("plain.json", &non_openapi_json(), "----BoundaryNonOpenApi");

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload non-OpenAPI JSON must return 400"
    );

    let body_text = read_body_string(resp.into_body()).await;
    assert!(
        body_text.contains("\u{7f3a}\u{5c11} openapi \u{5b57}\u{6bb5}"),
        "error message must contain '缺少 openapi 字段', got: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// 3. Upload OpenAPI JSON with empty paths -> 400
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the system to reject OpenAPI
// files that have no endpoints with a clear error, so that I know there is
// nothing to index.
// Covers: US-CORE-018, BE-D01 (EmptyPaths validation)

#[tokio::test]
async fn upload_openapi_empty_paths_returns_400() {
    let state = test_app_state().await;
    let app = create_api_routes(state);

    let req = upload_request(
        "empty_paths.json",
        &openapi_empty_paths_json(),
        "----BoundaryEmptyPaths",
    );

    let resp = app.oneshot(req).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "upload OpenAPI JSON with empty paths must return 400"
    );

    let body_text = read_body_string(resp.into_body()).await;
    assert!(
        body_text.contains("\u{6ca1}\u{6709}\u{53ef}\u{89e3}\u{6790}\u{7684} API \u{7aef}\u{70b9}"),
        "error message must contain '没有可解析的 API 端点', got: {body_text}"
    );
}

// ---------------------------------------------------------------------------
// 4. Upload OpenAPI JSON -> publish -> list shows published
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to upload an OpenAPI JSON file
// and then publish it, so that the API documentation becomes available for RAG
// retrieval via the document list.
// Covers: US-CORE-018, BE-D01 (full pipeline: upload -> publish -> list)

#[tokio::test]
async fn upload_openapi_then_publish_shows_in_document_list() {
    let state = test_app_state().await;
    let app = create_api_routes(state.clone());

    // Step 1: Upload valid OpenAPI JSON
    let req = upload_request(
        "petstore.json",
        &valid_openapi_json(),
        "----BoundaryPipeline",
    );
    let resp = app.oneshot(req).await.expect("send upload request");
    assert_eq!(resp.status(), StatusCode::OK, "upload must return 200");

    let upload_body = parse_json_body(resp.into_body()).await;
    let doc_id = upload_body["id"].as_str().expect("document id from upload");

    // Step 2: Publish the uploaded document
    let app = create_api_routes(state.clone());
    let req = auth_request(Method::PATCH, format!("/api/documents/{doc_id}/publish"));
    let resp = app.oneshot(req).await.expect("send publish request");
    assert_eq!(resp.status(), StatusCode::OK, "publish must return 200");

    let publish_body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        publish_body["status"], "published",
        "publish response status must be 'published'"
    );

    // Step 3: List documents and verify the document is published
    let app = create_api_routes(state);
    let req = auth_request(Method::GET, "/api/documents".to_string());
    let resp = app.oneshot(req).await.expect("send list request");
    assert_eq!(resp.status(), StatusCode::OK, "list must return 200");

    let list_body = parse_json_body(resp.into_body()).await;
    let documents = list_body["documents"].as_array().expect("documents array");
    let found = documents
        .iter()
        .find(|d| d["id"] == doc_id)
        .expect("find uploaded document in list");
    assert_eq!(
        found["status"], "published",
        "listed document must show 'published' status after publish"
    );
}
