//! Scenario tests for GET /api/chat/suggestions — match_locale pure function.
//!
//! Verifies the locale matching logic that powers the suggestions endpoint:
//! exact match -> longest prefix match -> "default" key -> empty vec.
//! All tests call `match_locale` directly with constructed config values.
//! No AppState construction needed.

use std::collections::HashMap;

use super::chat::match_locale;

// ---------------------------------------------------------------------------
// Scenario 1: Default group, no locale param
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 1; no locale provided -> default key lookup path.

#[test]
fn default_group_no_locale_returns_default_questions() {
    let config = Some(HashMap::from([
        (
            "default".to_string(),
            vec!["What is Rust?".to_string(), "How to install?".to_string()],
        ),
        ("en".to_string(), vec!["English question".to_string()]),
    ]));
    let result = match_locale(&config, None);

    assert_eq!(
        result,
        vec!["What is Rust?", "How to install?"],
        "without locale param, must return the 'default' group questions"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Exact locale match (case-insensitive)
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 2; exact match path.

#[test]
fn exact_locale_match_returns_matching_questions() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        (
            "zh-CN".to_string(),
            vec!["如何开始?".to_string(), "有什么功能?".to_string()],
        ),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["如何开始?", "有什么功能?"],
        "exact locale match must return the matching group's questions"
    );
}

// User Story: US-CORE-028
// Covers: Exact match is case-insensitive; locale "zh-cn" matches key "zh-CN".

#[test]
fn exact_locale_match_is_case_insensitive() {
    let config = Some(HashMap::from([(
        "zh-CN".to_string(),
        vec!["Chinese question".to_string()],
    )]));
    let result = match_locale(&config, Some("zh-cn"));

    assert_eq!(
        result,
        vec!["Chinese question"],
        "exact match must be case-insensitive: 'zh-cn' matches key 'zh-CN'"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3a: No prefix match, falls to default
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 3a; no prefix match for locale -> default path.

#[test]
fn no_prefix_match_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
        ("zh-CN".to_string(), vec!["CN Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("ja"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'ja' has no exact or prefix match, must fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3b: Prefix match to shorter key
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 3b; longest prefix match path.

#[test]
fn prefix_match_to_shorter_key() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
        ("zh-CN".to_string(), vec!["CN Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-TW"));

    assert_eq!(
        result,
        vec!["Chinese Q"],
        "'zh-TW' has no exact match; 'zh' is a prefix -> returns 'zh' group questions"
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: No suggested_questions configured
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 4; None config handling.

#[test]
fn no_config_returns_empty_array() {
    let result = match_locale(&None, Some("en"));

    assert!(
        result.is_empty(),
        "None config must return empty vec, not error"
    );
}

// User Story: US-CORE-028
// Covers: Empty HashMap is treated like None.

#[test]
fn empty_hashmap_returns_empty_array() {
    let config = Some(HashMap::new());
    let result = match_locale(&config, Some("en"));

    assert!(result.is_empty(), "empty HashMap must return empty vec");
}

// ---------------------------------------------------------------------------
// Scenario 5: Truncation at 10
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 5; truncation to MAX_SUGGESTIONS (10).

#[test]
fn more_than_ten_questions_truncated_to_ten() {
    let questions: Vec<String> = (1..=12).map(|i| format!("Question {i}")).collect();
    let config = Some(HashMap::from([("default".to_string(), questions)]));
    let result = match_locale(&config, None);

    assert_eq!(
        result.len(),
        10,
        "must truncate to 10 questions when config has more"
    );
    assert_eq!(result[0], "Question 1", "first question must be preserved");
    assert_eq!(result[9], "Question 10", "10th question must be preserved");
    assert!(
        !result.contains(&"Question 11".to_string()),
        "11th question must be truncated"
    );
    assert!(
        !result.contains(&"Question 12".to_string()),
        "12th question must be truncated"
    );
}

// ---------------------------------------------------------------------------
// Scenario 6: Empty locale string
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Locale validation; empty string fails format check, falls to default.

#[test]
fn empty_locale_string_treated_as_no_locale() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some(""));

    assert_eq!(
        result,
        vec!["Default Q"],
        "empty string locale fails validation, must fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 7: Invalid locale format
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Locale validation: digits in locale string rejected -> default fallback.

#[test]
fn invalid_locale_with_digits_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("abc123"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'abc123' contains digits, must be rejected and fall back to default"
    );
}

// User Story: US-CORE-028
// Covers: Locale validation: overly long locale string rejected -> default fallback.

#[test]
fn invalid_locale_too_long_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("en-US-extra-long"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'en-US-extra-long' exceeds max locale length, must be rejected and fall back to default"
    );
}

// User Story: US-CORE-028
// Covers: Locale validation: underscore separator rejected -> default fallback.

#[test]
fn invalid_locale_with_underscore_falls_to_default() {
    let config = Some(HashMap::from([(
        "default".to_string(),
        vec!["Default Q".to_string()],
    )]));
    let result = match_locale(&config, Some("en_US"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'en_US' uses underscore instead of hyphen, must be rejected and fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 8: Exact match but empty questions array
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Exact match returns whatever the config says, even if empty.

#[test]
fn exact_match_with_empty_array_returns_empty() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("fr".to_string(), vec![]),
    ]));
    let result = match_locale(&config, Some("fr"));

    assert!(
        result.is_empty(),
        "exact match to 'fr' must return its empty array, not fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 9: Multiple prefix matches, longest wins
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design BE-D01 spec "longest prefix match" -- most specific key wins.

#[test]
fn multiple_prefix_matches_longest_wins() {
    let config = Some(HashMap::from([
        ("z".to_string(), vec!["Z Q".to_string()]),
        ("zh".to_string(), vec!["ZH Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["ZH Q"],
        "'zh-CN' matches prefixes 'z' and 'zh'; 'zh' is longest, must win"
    );
}

// User Story: US-CORE-028
// Covers: Multiple prefix matches including 3-letter key; longest still wins.

#[test]
fn multiple_prefix_matches_with_three_char_key_longest_wins() {
    let config = Some(HashMap::from([
        ("z".to_string(), vec!["Z Q".to_string()]),
        ("zh".to_string(), vec!["ZH Q".to_string()]),
        ("zh-".to_string(), vec!["ZH-HYPHEN Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["ZH Q"],
        "'zh-CN' matches 'z', 'zh', but not 'zh-' (hyphen is not a prefix match since locale \
         validation rejects keys with trailing hyphen); 'zh' is longest valid prefix"
    );
}

// ---------------------------------------------------------------------------
// No match and no default key
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: When no locale match exists and no "default" key is configured,
//         the function returns an empty vec rather than panicking.

#[test]
fn no_match_and_no_default_key_returns_empty() {
    let config = Some(HashMap::from([
        ("en".to_string(), vec!["English Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("ja"));

    assert!(
        result.is_empty(),
        "no match for 'ja' and no 'default' key must return empty vec"
    );
}

// User Story: US-CORE-028
// Covers: None locale without default key returns empty vec.

#[test]
fn none_locale_without_default_returns_empty() {
    let config = Some(HashMap::from([(
        "en".to_string(),
        vec!["English Q".to_string()],
    )]));
    let result = match_locale(&config, None);

    assert!(
        result.is_empty(),
        "None locale without 'default' key must return empty vec"
    );
}

// ---------------------------------------------------------------------------
// Case-insensitive prefix matching
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Prefix matching is case-insensitive on both locale and key.

#[test]
fn prefix_match_is_case_insensitive() {
    let config = Some(HashMap::from([
        ("ZH".to_string(), vec!["Chinese Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-tw"));

    assert_eq!(
        result,
        vec!["Chinese Q"],
        "prefix match must be case-insensitive: 'zh-tw' matches key 'ZH'"
    );
}

// ===========================================================================
// GET /api/chat/suggestions HTTP handler scenario tests (BE-T03)
// ===========================================================================
//
// These tests exercise the public suggestions endpoint after `channelId` became
// required. They verify configured-channel validation, channel-level suggested
// question lookup, and the empty-array behavior for channels without configured
// questions.

use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

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

use rwiki_core::config::{ChannelConfig, ChannelsConfig, ChatConfig, RerankConfig};

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

const SUGGESTIONS_CHANNEL_A: &str = "help_center";
const SUGGESTIONS_CHANNEL_B: &str = "dev_docs";

/// Parse an Axum response body into a JSON value.
async fn parse_json_body(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, 1024 * 64)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Build a `ChannelsConfig` with one channel that has localized suggested questions
/// and one channel that has none.
fn suggestions_channels_config() -> ChannelsConfig {
    let mut channel_a_questions = HashMap::new();
    channel_a_questions.insert(
        "default".to_string(),
        vec!["How do I get started?".to_string()],
    );
    channel_a_questions.insert(
        "zh-CN".to_string(),
        vec!["如何开始？".to_string(), "如何联系客服？".to_string()],
    );

    let mut channels = HashMap::new();
    channels.insert(
        SUGGESTIONS_CHANNEL_A.to_string(),
        ChannelConfig {
            name: "Help Center".to_string(),
            system_prompt: None,
            suggested_questions: Some(channel_a_questions),
        },
    );
    channels.insert(
        SUGGESTIONS_CHANNEL_B.to_string(),
        ChannelConfig {
            name: "Developer Docs".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );

    ChannelsConfig { channels }
}

/// Build a minimal `AppState` suitable for suggestions endpoint tests.
/// The endpoint does not exercise the vector store or LLM, but AppState still
/// requires all fields to be populated.
async fn test_app_state_for_suggestions() -> Arc<AppState> {
    ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-suggestions-tests-only");
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
        api_token: "test-api-token-suggestions".to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: ChatConfig::default(),
        static_dir: None,
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: RerankConfig::default(),
        low_recall_config: None,
        channels_config: suggestions_channels_config(),
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// Build a GET request for `/api/chat/suggestions`.
fn suggestions_get_request(uri: &str) -> Request<Body> {
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("build request")
}

// User Story: support-multiple-website — A Widget or main-channel request with a
// valid `channelId` receives the channel-level suggested questions, localized by the
// `locale` query parameter.
// Covers: `suggestions` handler validates the channel, looks up the channel's
// `suggested_questions`, and uses `match_locale` to return the localized list.

#[tokio::test]
async fn suggestions_valid_channel_id_returns_localized_questions() {
    let state = test_app_state_for_suggestions().await;
    let app = create_api_routes(state);

    let req = suggestions_get_request(&format!(
        "/api/chat/suggestions?channelId={SUGGESTIONS_CHANNEL_A}&locale=zh-CN"
    ));
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "valid channel suggestions request must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["questions"],
        serde_json::json!(["如何开始？", "如何联系客服？"]),
        "must return the zh-CN questions configured for the channel, got {body}"
    );
}

// User Story: support-multiple-website — When a channel has no configured
// suggested questions, the endpoint returns an empty array. The Widget must
// not fall back to its own local list.
// Covers: `match_locale` receives `None` for the channel's questions and returns
// an empty Vec, which the handler serializes as `[]`.

#[tokio::test]
async fn suggestions_channel_without_configured_questions_returns_empty_array() {
    let state = test_app_state_for_suggestions().await;
    let app = create_api_routes(state);

    let req = suggestions_get_request(&format!(
        "/api/chat/suggestions?channelId={SUGGESTIONS_CHANNEL_B}&locale=en"
    ));
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "suggestions request for a channel without questions must return 200"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert_eq!(
        body["questions"],
        serde_json::json!([]),
        "must return an empty array when the channel has no suggested questions, got {body}"
    );
}

// User Story: support-multiple-website — The suggestions endpoint must reject
// requests that omit the required `channelId` query parameter.
// Covers: axum's `Query<SuggestionsQuery>` extractor rejects missing required
// fields with 400 Bad Request.

#[tokio::test]
async fn suggestions_missing_channel_id_returns_400() {
    let state = test_app_state_for_suggestions().await;
    let app = create_api_routes(state);

    let req = suggestions_get_request("/api/chat/suggestions?locale=en");
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "suggestions request without channelId must return 400"
    );
}

// User Story: support-multiple-website — An unconfigured `channelId` must be
// rejected by the suggestions endpoint.
// Covers: `suggestions` handler calls `channels_config.require_configured` and
// maps NotConfigured to 400 Bad Request.

#[tokio::test]
async fn suggestions_unconfigured_channel_id_returns_400() {
    let state = test_app_state_for_suggestions().await;
    let app = create_api_routes(state);

    let req = suggestions_get_request("/api/chat/suggestions?channelId=unknown-channel&locale=en");
    let resp = app.oneshot(req).await.expect("send request");

    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "suggestions request with unknown channelId must return 400"
    );

    let body = parse_json_body(resp.into_body()).await;
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("unknown-channel"),
        "error message must mention the unknown channel id, got {body}"
    );
}
