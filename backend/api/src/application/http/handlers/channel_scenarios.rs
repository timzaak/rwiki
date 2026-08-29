//! Channel list handler scenario tests.
//!
//! Verifies the public `GET /api/channels` endpoint:
//! - Returns configured channels as `{ id, name }` entries.
//! - Does not expose system prompts or suggested questions.
//! - Returns an empty array when no channels are configured.

use std::collections::HashMap;
use std::sync::{Arc, Once};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use rwiki_core::config::{ChannelConfig, ChannelsConfig};

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

const TEST_API_TOKEN: &str = "test-api-token-channel-list";

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

/// Build a minimal `AppState` with the supplied channel registry.
async fn test_app_state_with_channels(channels_config: ChannelsConfig) -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-channel-tests-only");
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
        chat_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: TEST_API_TOKEN.to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config: None,
        mcp_config: None,
        channels_config,
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build a `GET /api/channels` request.
fn channels_request() -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri("/api/channels")
        .body(Body::empty())
        .expect("build request")
}

/// Collect and parse a JSON response body.
async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("parse json")
}

fn sample_channels_config() -> ChannelsConfig {
    let mut questions = HashMap::new();
    questions.insert("default".to_string(), vec!["如何快速上手".to_string()]);
    questions.insert("en".to_string(), vec!["How to get started".to_string()]);

    let mut channels = HashMap::new();
    channels.insert(
        "help_center".to_string(),
        ChannelConfig {
            name: "Help Center".to_string(),
            system_prompt: Some("You are the Help Center assistant.".to_string()),
            suggested_questions: Some(questions),
        },
    );
    channels.insert(
        "developer_docs".to_string(),
        ChannelConfig {
            name: "Developer Docs".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );
    ChannelsConfig { channels }
}

// ---------------------------------------------------------------------------
// GET /api/channels scenarios
// ---------------------------------------------------------------------------

// User Story: support-multiple-website — As a frontend or widget, I want to
// discover configured channels so I can let users pick or validate a channel.
// Covers: Design 4.2.1 — `/api/channels` returns `{ id, name }` for every
//         configured channel and does not leak system prompts or questions.
#[tokio::test]
async fn list_channels_returns_id_and_name_without_prompts_or_suggestions() {
    let state = test_app_state_with_channels(sample_channels_config()).await;
    let app = create_api_routes(state);

    let resp = app.oneshot(channels_request()).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /api/channels must return 200"
    );

    let body = body_json(resp).await;
    let channels = body
        .get("channels")
        .expect("response should have channels")
        .as_array()
        .expect("channels should be an array");
    assert_eq!(
        channels.len(),
        2,
        "both configured channels should be returned"
    );

    let mut ids: Vec<&str> = channels
        .iter()
        .map(|s| {
            s.get("id")
                .expect("channel id")
                .as_str()
                .expect("id string")
        })
        .collect();
    ids.sort();
    assert_eq!(ids, vec!["developer_docs", "help_center"]);

    for channel in channels {
        assert!(channel.get("id").is_some(), "every entry must have id");
        assert!(channel.get("name").is_some(), "every entry must have name");
        assert!(
            channel.get("systemPrompt").is_none(),
            "system_prompt must not be exposed"
        );
        assert!(
            channel.get("suggestedQuestions").is_none(),
            "suggested_questions must not be exposed"
        );
    }
}

// User Story: support-multiple-website — Callers must be able to handle a
// registry with no channels even though startup validation normally prevents it.
// Covers: Design 4.2.1 — `/api/channels` returns `channels: []` for an empty registry.
#[tokio::test]
async fn list_channels_returns_empty_array_when_no_channels_configured() {
    let state = test_app_state_with_channels(ChannelsConfig::default()).await;
    let app = create_api_routes(state);

    let resp = app.oneshot(channels_request()).await.expect("send request");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET /api/channels must return 200 even with no channels"
    );

    let body = body_json(resp).await;
    let channels = body
        .get("channels")
        .and_then(|v| v.as_array())
        .expect("channels should be an array");
    assert!(
        channels.is_empty(),
        "channels array should be empty when nothing is configured"
    );
}
