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
