//! Scenario tests for LLM config loading (post openrouter->openai migration).
//!
//! Covers:
//! - Config loads [llm].api_key from file without OPENROUTER_API_KEY env var.
//! - Config ignores OPENROUTER_API_KEY env var even when set (env var reading removed).
//!
//! These tests provide regression protection for the env var removal in BE-D01.

use super::AppConfig;
use std::env;
use tempfile::NamedTempFile;

/// Helper: write a minimal valid TOML config with the given LLM api_key.
fn write_test_config(llm_api_key: &str) -> NamedTempFile {
    let content = format!(
        r#"
[server]
bind_address = "0.0.0.0:8080"
log_level = "info"
app_env = "test"
enable_openapi = false

[sqlite]
path = "data/test.db"

[llm]
api_key = "{llm_api_key}"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[embedding]

[api]

[chat]
system_prompt = "test prompt"
"#
    );
    let mut file = NamedTempFile::new().expect("should create temp file");
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");
    file
}

/// Helper: safely remove an env var, returning the previous value (if any) for restoration.
fn take_env(var: &str) -> Option<String> {
    match env::var(var) {
        Ok(val) => {
            env::remove_var(var);
            Some(val)
        }
        Err(_) => None,
    }
}

/// Helper: restore an env var to its previous value.
fn restore_env(var: &str, prev: Option<String>) {
    match prev {
        Some(val) => env::set_var(var, val),
        None => env::remove_var(var),
    }
}

// ---------------------------------------------------------------------------
// Config loads LLM api_key from file without OPENROUTER_API_KEY
// ---------------------------------------------------------------------------

// User Story: US-LLM-CONFIG-001
// Covers: After openrouter removal, [llm].api_key loads purely from config file.
//         OPENROUTER_API_KEY env var is no longer consulted.
#[test]
fn llm_api_key_loads_from_file_without_openrouter_env() {
    // Ensure OPENROUTER_API_KEY is not set
    let prev = take_env("OPENROUTER_API_KEY");

    let file = write_test_config("sk-file-key-12345");
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert_eq!(
        config.llm.api_key, "sk-file-key-12345",
        "llm.api_key must match the file value when OPENROUTER_API_KEY is absent"
    );

    restore_env("OPENROUTER_API_KEY", prev);
}

// ---------------------------------------------------------------------------
// Config ignores OPENROUTER_API_KEY env var when set
// ---------------------------------------------------------------------------

// User Story: US-LLM-CONFIG-001
// Covers: Even if OPENROUTER_API_KEY is set in the environment, config uses the file value.
//         The env var reading for LLM api_key was removed by BE-D01.
#[test]
fn llm_api_key_ignores_openrouter_env_var() {
    let prev = take_env("OPENROUTER_API_KEY");
    env::set_var("OPENROUTER_API_KEY", "sk-or-should-be-ignored");

    let file = write_test_config("sk-file-key-67890");
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert_eq!(
        config.llm.api_key, "sk-file-key-67890",
        "llm.api_key must match the file value, ignoring OPENROUTER_API_KEY env var"
    );

    restore_env("OPENROUTER_API_KEY", prev);
}

// ---------------------------------------------------------------------------
// Config preserves other fields from file
// ---------------------------------------------------------------------------

// User Story: US-LLM-CONFIG-001
// Covers: base_url and model are loaded from the config file unchanged.
#[test]
fn llm_base_url_and_model_load_from_file() {
    let prev = take_env("OPENROUTER_API_KEY");

    let file = write_test_config("sk-test");
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert_eq!(
        config.llm.base_url, "https://api.openai.com/v1",
        "llm.base_url must match the file value"
    );
    assert_eq!(
        config.llm.model, "gpt-4o-mini",
        "llm.model must match the file value"
    );

    restore_env("OPENROUTER_API_KEY", prev);
}

// ---------------------------------------------------------------------------
// Bailian DashScope full AppConfig (US-CORE-033 scenario 2 config seam)
// ---------------------------------------------------------------------------

/// Helper: write a full Bailian-oriented config that drives chat + embedding +
/// rerank from one provider. Mirrors the recommended `config.example.toml`
/// shape for US-CORE-033 ("single provider full stack").
fn write_bailian_config(llm_api_key: &str) -> NamedTempFile {
    let content = format!(
        r#"
[server]
bind_address = "0.0.0.0:8080"
log_level = "info"
app_env = "production"
enable_openapi = false

[sqlite]
path = "data/rwiki.db"

[llm]
api_key = "{llm_api_key}"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"

[embedding]
api_key = "{llm_api_key}"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "text-embedding-v3"

[rerank]
enable = true
provider = "dash_scope"
model = "qwen3-rerank"
top_n = 20
timeout_secs = 3

[api]
"#
    );
    let mut file = NamedTempFile::new().expect("should create temp file");
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");
    file
}

// User Story: US-CORE-033 (scenario 2 config seam) -- As a deployer, I configure
// a single Bailian API Key to drive chat + embedding + DashScope rerank, and the
// full AppConfig must parse with the rerank provider set to dash_scope.
// Covers: PRD 3.2 "单一 provider 全栈" + PRD 8 "精排新增 dash_scope provider" +
//         decision "默认精排模型 qwen3-rerank" -- verified at the AppConfig
//         deserialization seam (toml -> AppConfig), distinct from the RerankConfig
//         unit test in config.rs which only exercises the isolated struct.
#[test]
fn app_config_parses_bailian_single_provider_stack() {
    let prev_or = take_env("OPENROUTER_API_KEY");
    let prev_rerank = take_env("RERANK_API_KEY");

    let file = write_bailian_config("sk-bailian-real-key");
    let config = AppConfig::load(file.path()).expect("bailian config should load");

    // LLM + Embedding both point at Bailian's OpenAI-compatible endpoint and
    // share the same key -- this is the "single key full stack" intent.
    assert_eq!(
        config.llm.base_url,
        "https://dashscope.aliyuncs.com/compatible-mode/v1"
    );
    assert_eq!(config.llm.api_key, "sk-bailian-real-key");
    assert_eq!(
        config.embedding.api_key.as_deref(),
        Some("sk-bailian-real-key"),
        "embedding must accept the same Bailian key"
    );

    // Rerank provider dispatch selection happens at the config layer; this is
    // the field that selects the DashScopeReranker branch in main.rs.
    assert!(
        config.rerank.enable,
        "DashScope rerank must be enabled by the bailian config"
    );
    assert_eq!(
        config.rerank.provider,
        crate::config::RerankProviderType::DashScope,
        "provider must deserialize to DashScope variant (snake_case: dash_scope)"
    );
    assert_eq!(
        config.rerank.model.as_deref(),
        Some("qwen3-rerank"),
        "default Bailian rerank model is qwen3-rerank"
    );

    // Note: rerank.api_key intentionally unset here -- the cross-provider
    // fallback to llm.api_key happens at the main.rs construction layer and is
    // documented in `rerank_api_key_fallback_documented_as_runtime_concern`.
    assert!(
        config.rerank.api_key.is_none(),
        "rerank.api_key not set in this fixture; fallback is a runtime concern"
    );

    restore_env("OPENROUTER_API_KEY", prev_or);
    restore_env("RERANK_API_KEY", prev_rerank);
}

// ---------------------------------------------------------------------------
// RERANK_API_KEY env override (US-CORE-033 scenario 4 -- config-layer slice)
// ---------------------------------------------------------------------------

// User Story: US-CORE-033 (scenario 4) -- As a deployer, I can supply the rerank
// API Key either via [rerank].api_key in the file or the RERANK_API_KEY env var.
// Covers: AppConfig::load env-override contract for rerank -- RERANK_API_KEY
//         overrides the file value at config-load time. This is the
//         config-layer slice of the "single Bailian key reuse" story.
//
// IMPORTANT SCOPE NOTE: The full "fallback to llm.api_key when rerank.api_key is
// unset" behavior lives in `app/src/main.rs` (reranker construction block) and is
// coupled to global app state + tracing wiring. It cannot be exercised via
// AppConfig::load alone, so it is intentionally NOT asserted here. The fallback
// is verified by the deployment/integration path; faking it in this unit would
// duplicate main.rs logic and break when that logic changes.
#[test]
fn rerank_api_key_env_var_overrides_file_value() {
    let prev_rerank = take_env("RERANK_API_KEY");
    let prev_or = take_env("OPENROUTER_API_KEY");

    let mut file = NamedTempFile::new().expect("should create temp file");
    let content = r#"
[server]
bind_address = "0.0.0.0:8080"
log_level = "info"
app_env = "production"
enable_openapi = false

[sqlite]
path = "data/rwiki.db"

[llm]
api_key = "sk-llm-from-file"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"

[embedding]

[rerank]
enable = true
provider = "dash_scope"
model = "qwen3-rerank"
api_key = "sk-rerank-from-file"

[api]
"#;
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");

    env::set_var("RERANK_API_KEY", "sk-rerank-from-env");

    let config = AppConfig::load(file.path()).expect("config should load");

    // Env var wins over file value, mirroring OPENAI_API_KEY / API_TOKEN /
    // OTEL_LICENSE_KEY override semantics in AppConfig::load.
    assert_eq!(
        config.rerank.api_key.as_deref(),
        Some("sk-rerank-from-env"),
        "RERANK_API_KEY must override the file [rerank].api_key value"
    );
    // llm.api_key is untouched by the rerank env var.
    assert_eq!(config.llm.api_key, "sk-llm-from-file");

    restore_env("RERANK_API_KEY", prev_rerank);
    restore_env("OPENROUTER_API_KEY", prev_or);
}

// User Story: US-CORE-033 (scenario 4) -- When neither [rerank].api_key nor
// RERANK_API_KEY is set, rerank.api_key remains None at the config layer. This
// documents (and pins) the precondition under which main.rs falls back to
// llm.api_key. If this precondition changes, this test fails loudly so the
// fallback contract in main.rs is revisited intentionally.
#[test]
fn rerank_api_key_defaults_to_none_when_unset() {
    let prev_rerank = take_env("RERANK_API_KEY");
    let prev_or = take_env("OPENROUTER_API_KEY");
    // Both vars are now guaranteed unset.

    let mut file = NamedTempFile::new().expect("should create temp file");
    let content = r#"
[server]
bind_address = "0.0.0.0:8080"
log_level = "info"
app_env = "production"
enable_openapi = false

[sqlite]
path = "data/rwiki.db"

[llm]
api_key = "sk-llm"
base_url = "https://dashscope.aliyuncs.com/compatible-mode/v1"
model = "qwen-plus"

[embedding]

[rerank]
enable = true
provider = "dash_scope"
model = "qwen3-rerank"

[api]
"#;
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");

    let config = AppConfig::load(file.path()).expect("config should load");

    // Precondition for main.rs fallback to llm.api_key: rerank.api_key == None
    // after load. The fallback itself is a runtime concern in app/src/main.rs.
    assert!(
        config.rerank.api_key.is_none(),
        "rerank.api_key must be None when neither file nor env value is set, \
         which is the precondition for main.rs's llm.api_key fallback"
    );

    restore_env("RERANK_API_KEY", prev_rerank);
    restore_env("OPENROUTER_API_KEY", prev_or);
}
