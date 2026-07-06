//! Scenario tests for LLM config loading (post openrouter->openai migration).
//!
//! Covers:
//! - Config loads [llm].api_key from file without OPENROUTER_API_KEY env var.
//! - Config ignores OPENROUTER_API_KEY env var even when set (env var reading removed).
//!
//! These tests provide regression protection for the env var removal in BE-D01.

use super::{
    validate_historical_rows_have_site_id, AppConfig, SiteConfig, SiteValidationError, SitesConfig,
};
use std::collections::HashMap;
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
    // the field that selects the DashScopeReranker branch in main.rs. The
    // presence of the [rerank] section itself signals "enabled".
    let rerank = config
        .rerank
        .as_ref()
        .expect("bailian config has a [rerank] section, so rerank must be enabled");
    assert_eq!(
        rerank.provider,
        crate::config::RerankProviderType::DashScope,
        "provider must deserialize to DashScope variant (snake_case: dash_scope)"
    );
    assert_eq!(
        rerank.model.as_deref(),
        Some("qwen3-rerank"),
        "default Bailian rerank model is qwen3-rerank"
    );

    // Note: rerank.api_key intentionally unset here -- the cross-provider
    // fallback to llm.api_key happens at the main.rs construction layer and is
    // documented in `rerank_api_key_fallback_documented_as_runtime_concern`.
    assert!(
        rerank.api_key.is_none(),
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
    let rerank = config
        .rerank
        .as_ref()
        .expect("[rerank] section present, so rerank is enabled");
    assert_eq!(
        rerank.api_key.as_deref(),
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
provider = "dash_scope"
model = "qwen3-rerank"

[api]
"#;
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");

    let config = AppConfig::load(file.path()).expect("config should load");

    // Precondition for main.rs fallback to llm.api_key: rerank.api_key == None
    // after load. The fallback itself is a runtime concern in app/src/main.rs.
    let rerank = config
        .rerank
        .as_ref()
        .expect("[rerank] section present, so rerank is enabled");
    assert!(
        rerank.api_key.is_none(),
        "rerank.api_key must be None when neither file nor env value is set, \
         which is the precondition for main.rs's llm.api_key fallback"
    );

    restore_env("RERANK_API_KEY", prev_rerank);
    restore_env("OPENROUTER_API_KEY", prev_or);
}

// ---------------------------------------------------------------------------
// Post-answer suggested-questions switch (BE-T02 / US-CORE-037)
//
// The toggle `enable_post_answer_suggestions` lives on `ChatConfig`
// (`backend/core/src/config.rs` lines 129-133: `#[serde(default)] pub
// enable_post_answer_suggestions: bool`). It is reachable as
// `config.chat.enable_post_answer_suggestions`. The feature must be OFF by
// default in every existing deployment (design §1.4, §5.3) -- this is the core
// safety property: the SSE `suggestions` event must not appear for anyone who
// has not explicitly opted in. These four scenarios pin that property at the
// config-deserialization seam.
// ---------------------------------------------------------------------------

/// Helper: write a minimal valid TOML config with a custom `[chat]` section body.
///
/// `chat_body` is inserted verbatim between the `[chat]` header and the end of
/// the file. The required-fields skeleton (`[server]`, `[sqlite]`, `[llm]` with
/// `api_key`, `[embedding]`, `[api]`) is mirrored from `write_test_config` so
/// `AppConfig::load` accepts the file. To write a config with NO `[chat]`
/// section at all, pass `chat_body = ""` and the `[chat]` header is omitted.
fn write_chat_config(chat_body: &str) -> NamedTempFile {
    let chat_section = if chat_body.is_empty() {
        String::new()
    } else {
        format!("\n[chat]\n{chat_body}")
    };
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
api_key = "sk-chat-toggle-test"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[embedding]

[api]{chat_section}
"#
    );
    let mut file = NamedTempFile::new().expect("should create temp file");
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");
    file
}

// User Story: US-CORE-037 -- As a deployer, I expect the post-answer
// suggestions feature to be OFF when I have not explicitly opted in, so the
// SSE `suggestions` event never appears unexpectedly.
// Covers: The `#[serde(default)]` path on `ChatConfig.enable_post_answer_suggestions`
//         (`config.rs` line 132). A `[chat]` section that omits the field must
//         deserialize to `bool::default()` = `false`. This test FAILS if anyone
//         removes `#[serde(default)]` from the field (load would error) or flips
//         the default to `true` -- the load-bearing safety property of the feature.
#[test]
fn post_answer_suggestions_default_false_when_omitted_from_chat_section() {
    // `[chat]` block has only `system_prompt`; the toggle line is intentionally
    // absent, mirroring any pre-existing deployment that has not opted in.
    let file = write_chat_config(r#"system_prompt = "test prompt""#);
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert!(
        !config.chat.enable_post_answer_suggestions,
        "enable_post_answer_suggestions must default to false when omitted from [chat]"
    );
}

// User Story: US-CORE-037 -- As a deployer, I can opt into post-answer
// suggestions by setting `enable_post_answer_suggestions = true` in `[chat]`.
// Covers: The explicit-enable parse path. Verifies the field name and that the
//         type is a plain `bool` so a literal `true` round-trips through TOML.
//         Fails if the field is renamed or the type stops being a plain bool.
#[test]
fn post_answer_suggestions_parses_true_when_explicitly_enabled() {
    let file = write_chat_config(
        r#"system_prompt = "test prompt"
enable_post_answer_suggestions = true"#,
    );
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert!(
        config.chat.enable_post_answer_suggestions,
        "enable_post_answer_suggestions must parse as true when set explicitly in [chat]"
    );
}

// User Story: US-CORE-037 -- As a deployer with an existing config file that
// has no `[chat]` section at all, my deployment must keep loading and the
// post-answer suggestions feature must stay OFF (backward-compat, design §1.4).
// Covers: Verified contract -- `AppConfig.chat: ChatConfig` carries
//         `#[serde(default)]` (`backend/core/src/config.rs` lines 18-19), so a
//         missing `[chat]` section deserializes to `ChatConfig::default()`, and
//         BE-D01's `Default` impl sets `enable_post_answer_suggestions: false`
//         (`config.rs` line 161). This test FAILS loudly if anyone removes
//         `#[serde(default)]` from `AppConfig.chat` (load would error) or flips
//         the toggle default to `true`.
#[test]
fn post_answer_suggestions_missing_chat_section_is_backward_compatible_and_false() {
    // No `[chat]` section at all -- pass empty body so `write_chat_config`
    // omits the `[chat]` header entirely.
    let file = write_chat_config("");
    let config_result = AppConfig::load(file.path());

    assert!(
        config_result.is_ok(),
        "AppConfig must load even with no [chat] section (#[serde(default)] on AppConfig.chat)"
    );
    let config = config_result.expect("load result asserted Ok above");
    assert!(
        !config.chat.enable_post_answer_suggestions,
        "missing [chat] section must default the toggle to false via ChatConfig::default()"
    );
}

// User Story: US-CORE-037 -- As a developer, I rely on `ChatConfig::default()`
// to keep the feature OFF, so any code path that constructs a config without
// file input (tests, fresh AppState, fallbacks) cannot accidentally enable the
// SSE `suggestions` event.
// Covers: The `Default` impl entry BE-D01 added -- `enable_post_answer_suggestions:
//         false` (`config.rs` line 161). Fails if someone removes that explicit
//         initializer from `impl Default for ChatConfig`.
#[test]
fn post_answer_suggestions_default_impl_is_false() {
    let cfg = crate::config::ChatConfig::default();

    assert!(
        !cfg.enable_post_answer_suggestions,
        "ChatConfig::default() must produce enable_post_answer_suggestions == false"
    );
}

// ---------------------------------------------------------------------------
// LowRecallConfig enablement toggle (BE-T03 / US-CORE-038 + US-CORE-033)
//
// The low-recall logging feature's enablement lives on `AppConfig.low_recall:
// Option<LowRecallConfig>` (`backend/core/src/config.rs` lines 28-31:
// `#[serde(default)] pub low_recall: Option<LowRecallConfig>`). Per design §4.1
// the toggle mirrors the `[rerank]` section-presence convention established by
// commit 4b53c6d: an ABSENT `[low_recall]` section -> `None` (feature OFF); a
// PRESENT section (even empty) -> `Some(..)` (feature ON). The `threshold`
// field carries `#[serde(default = "default_low_recall_threshold")]` = 0.3
// (`config.rs` lines 325-327, 335). These three scenarios pin both properties
// at the config-deserialization seam.
// ---------------------------------------------------------------------------

/// Helper: write a minimal valid TOML config with a customizable `[low_recall]`
/// section, mirroring the `write_chat_config` skeleton (`[server]/[sqlite]/
/// [llm]/[embedding]/[api]`).
///
/// - `Some(threshold)` -> appends `[low_recall]\nthreshold = {threshold}\n`.
/// - `None`            -> appends `[low_recall]\n` alone (empty section,
///   triggers `#[serde(default)]` -> threshold = 0.3).
/// - To produce a config with NO `[low_recall]` section at all (the OFF case),
///   use `write_low_recall_config(None)` and then strip the empty section is
///   NOT done here -- callers that need the absent-section case build the file
///   inline (see `low_recall_disabled_when_section_absent`), keeping this
///   helper's contract single-purpose: "present section, with or without a
///   threshold".
fn write_low_recall_config(threshold: Option<f64>) -> NamedTempFile {
    let low_recall_section = match threshold {
        Some(v) => format!("\n[low_recall]\nthreshold = {v}\n"),
        None => "\n[low_recall]\n".to_string(),
    };
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
api_key = "sk-low-recall-test"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[embedding]

[api]{low_recall_section}
"#
    );
    let mut file = NamedTempFile::new().expect("should create temp file");
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");
    file
}

// User Story: US-CORE-038 -- As an operator, when I do not opt into low-recall
// logging by adding a `[low_recall]` section, the feature must stay OFF so no
// records are produced and chat latency/behavior is untouched.
// Also US-CORE-033 (config seam) -- the deserialization contract for the
// absent-section case.
// Covers: Design §4.1 (enablement = section presence, mirroring rerank commit
//         4b53c6d) + §5.1 (`#[serde(default)]` on `AppConfig.low_recall` ->
//         absent section deserializes to `None`). This test FAILS if anyone
//         removes `#[serde(default)]` from `AppConfig.low_recall` (load would
//         error on configs without the section, breaking every pre-existing
//         deployment) or flips the absent-section default to `Some(..)`
//         (silently enabling low-recall logging everywhere).
#[test]
fn low_recall_disabled_when_section_absent() {
    let prev_or = take_env("OPENROUTER_API_KEY");

    // Minimal config with NO `[low_recall]` section at all -- mirrors any
    // deployment that has not opted in.
    let mut file = NamedTempFile::new().expect("should create temp file");
    let content = r#"
[server]
bind_address = "0.0.0.0:8080"
log_level = "info"
app_env = "test"
enable_openapi = false

[sqlite]
path = "data/test.db"

[llm]
api_key = "sk-low-recall-absent"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[embedding]

[api]
"#;
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");

    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    assert!(
        config.low_recall.is_none(),
        "low_recall must be None when [low_recall] section is absent (feature OFF by default)"
    );

    restore_env("OPENROUTER_API_KEY", prev_or);
}

// User Story: US-CORE-038 -- As an operator, I opt into low-recall logging
// simply by adding a `[low_recall]` section (even with no fields), and the
// feature turns ON with the documented default threshold 0.3 so I do not have
// to guess a value on first enable.
// Also US-CORE-033 (config seam) -- the section-presence enablement contract.
// Covers: Design §4.1 (section presence = enablement) + §5.1 (`LowRecallConfig`
//         `#[serde(default = "default_low_recall_threshold")]` -> 0.3 when the
//         field is omitted; `default_low_recall_threshold()` returns 0.3 per
//         `config.rs` lines 325-327). This test FAILS if anyone changes the
//         enablement model (e.g. adds an `enable` flag) or changes the default
//         threshold away from 0.3 without revisiting the calibration baseline.
#[test]
fn low_recall_enabled_when_section_present() {
    let prev_or = take_env("OPENROUTER_API_KEY");

    // Empty `[low_recall]` section (no threshold field) -> Some(..) with the
    // serde default threshold.
    let file = write_low_recall_config(None);
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    let low_recall = config
        .low_recall
        .as_ref()
        .expect("present [low_recall] section (even empty) must deserialize to Some(..)");

    assert_eq!(
        low_recall.threshold, 0.3,
        "empty [low_recall] section must fall back to the serde default threshold of 0.3"
    );

    restore_env("OPENROUTER_API_KEY", prev_or);
}

// User Story: US-CORE-038 -- As an operator, I can tune the low-recall
// threshold by setting `threshold` in `[low_recall]`, and the configured value
// is parsed verbatim (no clamping/surprise coercion) so calibration is
// predictable.
// Also US-CORE-033 (config seam) -- the explicit-value parse path.
// Covers: Design §5.1 (`LowRecallConfig.threshold: f64` round-trips through
//         TOML). This test FAILS if the field is renamed, the type stops being
//         a plain `f64`, or anyone adds silent clamping that would mask a
//         calibration typo.
#[test]
fn low_recall_threshold_explicit_value_parsed() {
    let prev_or = take_env("OPENROUTER_API_KEY");

    // Explicit non-default threshold 0.45 (chosen off the 0.3 default so a
    // default-leak would be caught).
    let file = write_low_recall_config(Some(0.45));
    let config = AppConfig::load(file.path()).expect("config should load from temp file");

    let low_recall = config
        .low_recall
        .expect("present [low_recall] section must deserialize to Some(..)");

    assert_eq!(
        low_recall.threshold, 0.45,
        "explicitly set [low_recall].threshold must parse verbatim (no clamping/coercion)"
    );

    restore_env("OPENROUTER_API_KEY", prev_or);
}

// ---------------------------------------------------------------------------
// Site configuration scenarios (BE-T01)
//
// Covers: configured site parsing, default representation of omitted fields,
//         startup rejection preconditions, and historical-row validation.
// ---------------------------------------------------------------------------

/// Helper: write a minimal valid TOML config with the given `[sites]` body.
fn write_app_config_with_sites(sites_body: &str) -> NamedTempFile {
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
api_key = "sk-site-config-test"
base_url = "https://api.openai.com/v1"
model = "gpt-4o-mini"

[embedding]

[api]

[chat]
system_prompt = "test prompt"

{sites_body}
"#
    );
    let mut file = NamedTempFile::new().expect("should create temp file");
    std::io::Write::write_all(&mut file, content.as_bytes()).expect("should write config content");
    file
}

// User Story: support-multiple-website — As a deployer, I configure multiple
// sites with names, per-site system prompts, and localized suggested questions.
// Covers: Design 5.1 — full AppConfig parses `[sites.<id>]` with all optional
//         fields present on one site and absent on another.
#[test]
fn multiple_sites_parse_with_names_prompts_and_localized_suggestions() {
    let sites = r#"
[sites.help_center]
name = "Help Center"
system_prompt = "You are the Help Center assistant."
[sites.help_center.suggested_questions]
default = ["如何快速上手", "支持哪些文件格式"]
zh-CN = ["如何快速上手", "支持哪些文件格式", "如何联系客服"]
en = ["How to get started", "What file formats are supported"]

[sites.developer_docs]
name = "Developer Docs"
"#;
    let file = write_app_config_with_sites(sites);
    let config = AppConfig::load(file.path()).expect("config with sites should load");

    assert_eq!(
        config.sites.sites.len(),
        2,
        "both configured sites should parse"
    );

    let help = config
        .sites
        .get("help_center")
        .expect("help_center should exist");
    assert_eq!(help.name, "Help Center");
    assert_eq!(
        help.system_prompt.as_deref(),
        Some("You are the Help Center assistant.")
    );
    let help_qs = help
        .suggested_questions
        .as_ref()
        .expect("help_center should have suggested_questions");
    assert_eq!(
        help_qs.get("default").unwrap(),
        &vec!["如何快速上手", "支持哪些文件格式"]
    );
    assert_eq!(
        help_qs.get("zh-CN").unwrap(),
        &vec!["如何快速上手", "支持哪些文件格式", "如何联系客服"]
    );
    assert_eq!(
        help_qs.get("en").unwrap(),
        &vec!["How to get started", "What file formats are supported"]
    );

    let dev = config
        .sites
        .get("developer_docs")
        .expect("developer_docs should exist");
    assert_eq!(dev.name, "Developer Docs");
    assert!(
        dev.system_prompt.is_none(),
        "omitted system_prompt should be None"
    );
    assert!(
        dev.suggested_questions.is_none(),
        "omitted suggested_questions should be None"
    );
}

// User Story: support-multiple-website — As a deployer, I can declare a site
// with only a name and rely on the global prompt / empty suggestions.
// Covers: Design 5.1 — omitted `system_prompt` and `suggested_questions` stay
//         `Option::None`; tests must not invent fallback values.
#[test]
fn omitted_site_prompt_and_suggestions_represent_defaults() {
    let sites = r#"
[sites.minimal]
name = "Minimal Site"
"#;
    let file = write_app_config_with_sites(sites);
    let config = AppConfig::load(file.path()).expect("config should load");

    let site = config.sites.get("minimal").expect("minimal should exist");
    assert_eq!(site.name, "Minimal Site");
    assert!(
        site.system_prompt.is_none(),
        "site without prompt must default to None, not a synthesized fallback"
    );
    assert!(
        site.suggested_questions.is_none(),
        "site without suggested_questions must default to None"
    );
}

// User Story: support-multiple-website backward compatibility — existing
// deployments without a `[sites]` section must still load.
// Covers: `AppConfig.sites` uses `#[serde(default)]`; missing section
//         deserializes to an empty `SitesConfig`.
#[test]
fn app_config_without_sites_section_defaults_to_empty_registry() {
    let file = write_app_config_with_sites("");
    let config = AppConfig::load(file.path()).expect("config without sites should load");

    assert!(
        config.sites.is_empty(),
        "missing [sites] section must default to empty registry"
    );
}

// User Story: support-multiple-website — The service must fail loudly at
// startup when no site is configured, so operators notice misconfiguration.
// Covers: app/src/main.rs checks `config.sites.is_empty()` and returns Err.
//         AppConfig::load intentionally does NOT reject empty sites (backward
//         compat); this scenario pins the predicate that triggers startup failure.
#[test]
fn empty_sites_registry_is_the_startup_failure_precondition() {
    let sites = SitesConfig::default();
    assert!(
        sites.is_empty(),
        "empty registry is the condition main.rs checks before returning Err"
    );
    assert!(sites.list_metadata().is_empty());
}

// User Story: support-multiple-website — The public site list must expose only
// the information needed by callers (id + name), never system prompts.
// Covers: `SitesConfig::list_metadata` returns `SiteMetadata` with only id/name.
#[test]
fn site_list_metadata_excludes_system_prompt_and_suggested_questions() {
    let mut questions = HashMap::new();
    questions.insert("default".to_string(), vec!["q1".to_string()]);

    let mut sites = SitesConfig::default();
    sites.sites.insert(
        "help_center".to_string(),
        SiteConfig {
            name: "Help Center".to_string(),
            system_prompt: Some("secret prompt".to_string()),
            suggested_questions: Some(questions),
        },
    );

    let metadata = sites.list_metadata();
    assert_eq!(metadata.len(), 1);
    assert_eq!(metadata[0].id, "help_center");
    assert_eq!(metadata[0].name, "Help Center");

    // SiteMetadata only exposes id/name; serializing it must not contain the
    // site-level private fields.
    let json = serde_json::to_value(&metadata[0]).expect("serialize metadata");
    assert!(json.get("system_prompt").is_none());
    assert!(json.get("suggested_questions").is_none());
}

// User Story: support-multiple-website — Handlers validate incoming siteIds.
// Covers: `SitesConfig::require_configured` rejects empty/whitespace/unknown ids
//         and normalizes whitespace around configured ids.
#[test]
fn require_configured_normalizes_and_rejects_invalid_site_ids() {
    let mut sites = SitesConfig::default();
    sites.sites.insert(
        "help_center".to_string(),
        SiteConfig {
            name: "Help Center".to_string(),
            system_prompt: None,
            suggested_questions: None,
        },
    );

    assert_eq!(
        sites.require_configured("help_center"),
        Ok("help_center"),
        "configured site should resolve"
    );
    assert_eq!(
        sites.require_configured("  help_center  "),
        Ok("help_center"),
        "whitespace around configured id should be normalized"
    );
    assert_eq!(
        sites.require_configured(""),
        Err(SiteValidationError::Empty),
        "empty site id should fail"
    );
    assert_eq!(
        sites.require_configured("unknown"),
        Err(SiteValidationError::NotConfigured("unknown".to_string())),
        "unknown site id should fail"
    );
}

// User Story: support-multiple-website — Each site may override the global
// system prompt; omitting or emptying the override must fall back to global.
// Covers: `SitesConfig::resolved_system_prompt` contract.
#[test]
fn resolved_system_prompt_uses_site_value_or_global_fallback() {
    let mut sites = SitesConfig::default();
    sites.sites.insert(
        "help_center".to_string(),
        SiteConfig {
            name: "Help Center".to_string(),
            system_prompt: Some("Site prompt.".to_string()),
            suggested_questions: None,
        },
    );
    sites.sites.insert(
        "empty_prompt".to_string(),
        SiteConfig {
            name: "Empty Prompt".to_string(),
            system_prompt: Some("".to_string()),
            suggested_questions: None,
        },
    );

    let global = "Global prompt.";
    assert_eq!(
        sites.resolved_system_prompt(Some("help_center"), global),
        "Site prompt."
    );
    assert_eq!(
        sites.resolved_system_prompt(Some("empty_prompt"), global),
        global,
        "empty site prompt must fall back to global"
    );
    assert_eq!(
        sites.resolved_system_prompt(Some("missing"), global),
        global
    );
    assert_eq!(sites.resolved_system_prompt(None, global), global);
}

// User Story: support-multiple-website — Existing data must be backfilled with
// site_id before the service starts; otherwise startup must fail loud.
// Covers: `validate_historical_rows_have_site_id` detects NULL site_id rows.
#[tokio::test]
async fn startup_validation_rejects_historical_rows_with_null_site_id() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    let sqlite = tokio_rusqlite::Connection::from(conn);

    sqlite
        .call(|conn| {
            conn.execute(
                "CREATE TABLE documents (id TEXT PRIMARY KEY, file_name TEXT NOT NULL, site_id TEXT)",
                [],
            )?;
            conn.execute(
                "INSERT INTO documents (id, file_name) VALUES (?1, ?2)",
                rusqlite::params!["doc-1", "legacy.txt"],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("seed table");

    let err = validate_historical_rows_have_site_id(&sqlite, &["documents"])
        .await
        .expect_err("null site_id rows must block startup");
    assert!(
        err.to_string().contains("missing site_id"),
        "error should mention missing site_id: {err}"
    );
}

// User Story: support-multiple-website — Startup validation must not crash on
// tables that have not yet received the `site_id` column (e.g. during staged
// migrations).
// Covers: `validate_historical_rows_have_site_id` skips tables without site_id.
#[tokio::test]
async fn startup_validation_skips_tables_without_site_id_column() {
    let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    let sqlite = tokio_rusqlite::Connection::from(conn);

    sqlite
        .call(|conn| {
            conn.execute("CREATE TABLE low_recall_records (id TEXT PRIMARY KEY)", [])?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .expect("create table");

    validate_historical_rows_have_site_id(&sqlite, &["low_recall_records"])
        .await
        .expect("tables without site_id column should be skipped");
}
