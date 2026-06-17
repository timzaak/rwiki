use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::path::Path;

/// 应用配置
///
/// 从 TOML 配置文件加载，包含服务器、SQLite、LLM、Embedding、API Token、Chat 和 OTel 七个部分的配置。
/// 配置文件示例见 config/config.example.toml。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub sqlite: SqliteConfig,
    pub llm: LlmConfig,
    pub embedding: EmbeddingConfig,
    #[serde(default)]
    pub api: ApiTokenConfig,
    #[serde(default)]
    pub chat: ChatConfig,
    #[serde(default)]
    pub otel: OtelConfig,
    #[serde(default)]
    pub retrieval: RetrievalConfig,
    #[serde(default)]
    pub rerank: RerankConfig,
}

impl AppConfig {
    /// 从 TOML 文件加载配置，API Key 支持环境变量覆盖
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let mut config: AppConfig = toml::from_str(&content)?;
        if let Ok(key) = env::var("OPENAI_API_KEY") {
            config.embedding.api_key = Some(key);
        }
        if let Ok(t) = env::var("API_TOKEN") {
            config.api.token = t;
        }
        if let Ok(key) = env::var("OTEL_LICENSE_KEY") {
            config.otel.license_key = key;
        }
        if let Ok(key) = env::var("RERANK_API_KEY") {
            config.rerank.api_key = Some(key);
        }
        Ok(config)
    }
}

/// 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// 监听地址，如 "0.0.0.0:8080"
    pub bind_address: String,
    /// 日志级别：trace, debug, info, warn, error
    pub log_level: String,
    /// 运行环境：development, test, production
    pub app_env: String,
    /// 是否启用 OpenAPI/Swagger 文档
    /// 设为 true 后可访问 /swagger 查看 API 文档
    pub enable_openapi: bool,
    /// 静态文件目录路径（含 widget JS 等）
    /// 为空或未配置时不托管静态文件
    #[serde(default)]
    pub static_dir: Option<String>,
}

/// SQLite 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    /// SQLite 数据库文件路径，默认 "data/rwiki.db"
    pub path: String,
}

/// LLM 配置（OpenAI 兼容 provider）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    /// LLM API Key
    pub api_key: String,
    /// LLM API base URL
    pub base_url: String,
    /// 模型名称
    pub model: String,
}

/// Embedding 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// API Key
    #[serde(default)]
    pub api_key: Option<String>,
    /// Optional base URL override
    pub base_url: Option<String>,
    /// Model name (default: "text-embedding-3-small")
    #[serde(default)]
    pub model: Option<String>,
    /// 向量维度，可选。不设置时使用模型默认维度。
    /// BigModel Embedding-3 支持 256/512/1024/2048。
    #[serde(default)]
    pub dimensions: Option<usize>,
}

/// 聊天配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatConfig {
    /// 自定义系统提示词（角色描述 + 行为规则）。
    /// 省略时使用内置默认值。
    /// 上下文（RAG 检索结果）由系统自动拼接在提示词之后。
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// 滑动窗口大小：保留最近 N 条消息用于上下文
    #[serde(default = "default_sliding_window_size")]
    pub sliding_window_size: usize,
    /// 压缩阈值：消息数超过此值时触发压缩检查
    #[serde(default = "default_compact_threshold")]
    pub compact_threshold: usize,
    /// Token 预算：估算 token 数超过此值时触发压缩
    #[serde(default = "default_token_budget")]
    pub token_budget: usize,
    /// 知识库文档的主要语言（如 "Chinese"、"English"）。
    /// 设置后，查询改写会将用户查询翻译为该语言，以提升跨语言检索命中率。
    /// 省略时不做语言转换。
    #[serde(default)]
    pub content_language: Option<String>,
    /// 推荐问题（按语言分组）。
    /// Key 为语言标签（如 "default"、"zh-CN"、"en"），Value 为该语言的推荐问题列表。
    /// 最多展示 10 条。
    #[serde(default)]
    pub suggested_questions: Option<HashMap<String, Vec<String>>>,
    /// 回答完成后是否生成并下发后续推荐问题（SSE `suggestions` 事件）。
    /// 开启后，每次主回答完成后会追加一次非流式 LLM 调用（带超时与 token 预算）。
    /// 默认关闭；省略该字段时按关闭处理（向后兼容）。
    #[serde(default)]
    pub enable_post_answer_suggestions: bool,
}

fn default_system_prompt() -> String {
    ChatConfig::DEFAULT_SYSTEM_PROMPT.to_string()
}

fn default_sliding_window_size() -> usize {
    6
}

fn default_compact_threshold() -> usize {
    8
}

fn default_token_budget() -> usize {
    8000
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            system_prompt: Self::DEFAULT_SYSTEM_PROMPT.to_string(),
            sliding_window_size: default_sliding_window_size(),
            compact_threshold: default_compact_threshold(),
            token_budget: default_token_budget(),
            content_language: None,
            suggested_questions: None,
            enable_post_answer_suggestions: false,
        }
    }
}

impl ChatConfig {
    /// 默认系统提示词，与原硬编码值保持一致
    pub const DEFAULT_SYSTEM_PROMPT: &str = "\
你是一个知识库助手。请根据以下上下文回答用户问题。\n\
规则：\n\
1. 如果上下文中没有相关信息，请明确告知用户。\n\
2. 引用信息时，请使用 [Source N] 格式标注来源编号，N 对应上下文中的 document index。\n\
3. 如果来源有链接，请在引用中附上链接，让用户可以直接访问原始页面。\n\
4. 如果多个来源提供相同信息，合并引用，如 [Source 1][Source 2]。\n\
5. 如果来源之间信息冲突，说明冲突并分别引用相关来源。\n\
6. 如果来源标注了语言（Locale），请在引用中提及信息的语言。";
}

fn default_search_top_k_per_query() -> usize {
    5
}

fn default_max_context_chunks() -> usize {
    12
}

/// RAG 检索配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalConfig {
    /// 每个查询变体召回的候选数量（默认 5）
    #[serde(default = "default_search_top_k_per_query")]
    pub search_top_k_per_query: usize,
    /// 最终送入 LLM 上下文的最大 chunk 数（默认 12）
    #[serde(default = "default_max_context_chunks")]
    pub max_context_chunks: usize,
}

impl Default for RetrievalConfig {
    fn default() -> Self {
        Self {
            search_top_k_per_query: default_search_top_k_per_query(),
            max_context_chunks: default_max_context_chunks(),
        }
    }
}

/// API Token 配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiTokenConfig {
    #[serde(default)]
    pub token: String,
}

/// OpenTelemetry OTLP 导出配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelConfig {
    /// OTLP gRPC endpoint（如 "https://tracing-analysis-dc-hz.aliyuncs.com:8090"）
    /// 为空或未配置时不启用 OTLP
    #[serde(default)]
    pub endpoint: String,
    /// 鉴权 token，可通过 OTEL_LICENSE_KEY 环境变量覆盖
    #[serde(default)]
    pub license_key: String,
    /// 服务名称，默认 "rwiki-backend"
    #[serde(default = "default_service_name")]
    pub service_name: String,
}

fn default_service_name() -> String {
    "rwiki-backend".to_string()
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            license_key: String::new(),
            service_name: default_service_name(),
        }
    }
}

/// Rerank provider 类型
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RerankProviderType {
    #[default]
    OpenRouter,
    BigModel,
    DashScope,
}

fn default_top_n() -> usize {
    20
}

fn default_timeout_secs() -> u64 {
    3
}

/// Rerank 精排配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    /// 是否启用 rerank，默认 false
    #[serde(default)]
    pub enable: bool,
    /// Rerank provider 类型
    #[serde(default)]
    pub provider: RerankProviderType,
    /// 模型名称
    #[serde(default)]
    pub model: Option<String>,
    /// 送入 rerank 的最大候选数量（默认 20）
    #[serde(default = "default_top_n")]
    pub top_n: usize,
    /// Rerank API 调用超时（秒，默认 3）
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// 独立 API Key
    #[serde(default)]
    pub api_key: Option<String>,
    /// 可选显式 base_url 覆盖；未设置时使用 provider 默认端点。
    #[serde(default)]
    pub base_url: Option<String>,
}

impl Default for RerankConfig {
    fn default() -> Self {
        Self {
            enable: false,
            provider: RerankProviderType::default(),
            model: None,
            top_n: default_top_n(),
            timeout_secs: default_timeout_secs(),
            api_key: None,
            base_url: None,
        }
    }
}

#[cfg(test)]
#[path = "config_scenarios.rs"]
mod config_scenarios;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_config_dimensions_backward_compatible() {
        // Config without dimensions field (existing deployments)
        let toml_without = r#"
            api_key = "sk-test"
            base_url = "https://api.openai.com/v1"
            model = "text-embedding-3-small"
        "#;
        let config: EmbeddingConfig =
            toml::from_str(toml_without).expect("config without dimensions should deserialize");
        assert_eq!(
            config.dimensions, None,
            "missing dimensions should default to None"
        );

        // Config with dimensions field (new deployments)
        let toml_with = r#"
            api_key = "sk-test"
            base_url = "https://open.bigmodel.cn/api/paas/v4"
            model = "embedding-3"
            dimensions = 2048
        "#;
        let config: EmbeddingConfig =
            toml::from_str(toml_with).expect("config with dimensions should deserialize");
        assert_eq!(
            config.dimensions,
            Some(2048),
            "dimensions should parse as Some(2048)"
        );
    }

    #[test]
    fn chat_config_parses_custom_prompt() {
        let toml_str = r#"
            system_prompt = "Custom prompt here"
        "#;
        let config: ChatConfig = toml::from_str(toml_str).expect("chat config should deserialize");
        assert_eq!(config.system_prompt, "Custom prompt here");
    }

    #[test]
    fn chat_config_default_prompt_matches_hardcoded() {
        assert!(
            ChatConfig::DEFAULT_SYSTEM_PROMPT.contains("知识库助手"),
            "DEFAULT_SYSTEM_PROMPT should contain the role description"
        );
        assert!(
            ChatConfig::DEFAULT_SYSTEM_PROMPT.contains("引用信息时"),
            "DEFAULT_SYSTEM_PROMPT should contain citation rule"
        );
    }

    #[test]
    fn chat_config_missing_section_uses_defaults() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"

            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]
        "#;
        let result = toml::from_str::<AppConfig>(toml_str);
        assert!(
            result.is_ok(),
            "missing [chat] section should use defaults (backward compatible)"
        );
        let config = result.unwrap();
        assert_eq!(
            config.chat.system_prompt,
            ChatConfig::DEFAULT_SYSTEM_PROMPT,
            "missing [chat] should default system_prompt"
        );
        assert_eq!(config.chat.sliding_window_size, 6);
        assert_eq!(config.chat.compact_threshold, 8);
        assert_eq!(config.chat.token_budget, 8000);
        assert_eq!(config.retrieval.search_top_k_per_query, 5);
        assert_eq!(config.retrieval.max_context_chunks, 12);
    }

    #[test]
    fn chat_config_multiline_prompt_preserves_newlines() {
        let toml_str = r#"
            system_prompt = """你是一个专业的技术文档助手。
规则：
1. 使用代码示例。
2. 引用来源。"""
        "#;
        let config: ChatConfig =
            toml::from_str(toml_str).expect("multi-line chat config should deserialize");
        assert!(
            config.system_prompt.contains('\n'),
            "multi-line prompt should preserve newlines"
        );
        assert!(
            config.system_prompt.contains("技术文档助手"),
            "multi-line prompt should preserve content"
        );
    }

    #[test]
    fn otel_config_default_disables_tracing() {
        let config = OtelConfig::default();
        assert!(
            config.endpoint.is_empty(),
            "default endpoint should be empty"
        );
        assert!(
            config.license_key.is_empty(),
            "default license_key should be empty"
        );
        assert_eq!(config.service_name, "rwiki-backend");
    }

    #[test]
    fn otel_config_parses_all_fields() {
        let toml_str = r#"
            endpoint = "https://tracing.example.com:8090"
            license_key = "test-key"
            service_name = "custom-service"
        "#;
        let config: OtelConfig = toml::from_str(toml_str).expect("otel config should deserialize");
        assert_eq!(config.endpoint, "https://tracing.example.com:8090");
        assert_eq!(config.license_key, "test-key");
        assert_eq!(config.service_name, "custom-service");
    }

    #[test]
    fn otel_config_partial_uses_defaults() {
        let toml_str = r#"
            endpoint = "https://tracing.example.com:8090"
        "#;
        let config: OtelConfig =
            toml::from_str(toml_str).expect("partial otel config should deserialize");
        assert_eq!(config.endpoint, "https://tracing.example.com:8090");
        assert!(config.license_key.is_empty());
        assert_eq!(config.service_name, "rwiki-backend");
    }

    #[test]
    fn app_config_without_otel_uses_defaults() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"

            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]

            [chat]
            system_prompt = "test"
        "#;
        let config: AppConfig =
            toml::from_str(toml_str).expect("config without [otel] should deserialize");
        assert!(config.otel.endpoint.is_empty());
        assert_eq!(config.otel.service_name, "rwiki-backend");
    }

    // Covers: Retrieval config defaults — missing [retrieval] preserves existing search behavior.
    #[test]
    fn retrieval_config_default_preserves_existing_limits() {
        let config = RetrievalConfig::default();
        assert_eq!(
            config.search_top_k_per_query, 5,
            "default per-query search top-k should preserve the previous hardcoded value"
        );
        assert_eq!(
            config.max_context_chunks, 12,
            "default max context chunks should preserve the previous hardcoded value"
        );
    }

    // Covers: Retrieval config parsing — admins can tune search and context limits from TOML.
    #[test]
    fn retrieval_config_parses_all_fields() {
        let toml_str = r#"
            search_top_k_per_query = 8
            max_context_chunks = 16
        "#;
        let config: RetrievalConfig =
            toml::from_str(toml_str).expect("retrieval config should deserialize");
        assert_eq!(config.search_top_k_per_query, 8);
        assert_eq!(config.max_context_chunks, 16);
    }

    // Covers: PRD config-driven enablement — license_key field parsing within full AppConfig TOML.
    // User Story: OTLP config integration — [otel].license_key parsed from TOML alongside other sections.
    #[test]
    fn otel_config_parses_license_key_from_toml() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"

            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]

            [chat]
            system_prompt = "test"

            [otel]
            endpoint = "https://tracing.example.com:8090"
            license_key = "file-value"
        "#;
        let config: AppConfig = toml::from_str(toml_str).expect("config should deserialize");
        assert_eq!(config.otel.license_key, "file-value");
    }

    // Covers: Design 5.1 OtelConfig struct — all fields parse correctly within AppConfig context.
    // User Story: OTLP config integration — endpoint, license_key, and service_name all deserialize.
    #[test]
    fn otel_config_all_fields_in_app_config() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"

            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]

            [chat]
            system_prompt = "test"

            [otel]
            endpoint = "https://tracing.example.com:8090"
            license_key = "my-key"
            service_name = "custom-name"
        "#;
        let config: AppConfig =
            toml::from_str(toml_str).expect("config with otel should deserialize");
        assert_eq!(config.otel.endpoint, "https://tracing.example.com:8090");
        assert_eq!(config.otel.license_key, "my-key");
        assert_eq!(config.otel.service_name, "custom-name");
    }

    // --- content_language config parsing tests (BE-T01) ---

    // Covers: Design 5.1 — content_language parses from TOML with English value.
    // User Story: Query language aware rewrite — English content_language deserializes correctly.
    #[test]
    fn chat_config_parses_content_language_english() {
        let toml_str = r#"
            content_language = "English"
        "#;
        let config: ChatConfig = toml::from_str(toml_str).expect("chat config should deserialize");
        assert_eq!(
            config.content_language,
            Some("English".to_string()),
            "content_language should parse as Some(\"English\")"
        );
    }

    // Covers: Design 5.1 — content_language parses from TOML with Chinese value.
    // User Story: Query language aware rewrite — Chinese content_language deserializes correctly.
    #[test]
    fn chat_config_parses_content_language_chinese() {
        let toml_str = r#"
            content_language = "中文"
        "#;
        let config: ChatConfig = toml::from_str(toml_str).expect("chat config should deserialize");
        assert_eq!(
            config.content_language,
            Some("中文".to_string()),
            "content_language should parse as Some(\"中文\")"
        );
    }

    // Covers: Design 5.1 — content_language parses empty string (handler layer filters to None).
    // User Story: Query language aware rewrite — empty string stored as-is; handler filters it.
    #[test]
    fn chat_config_parses_content_language_empty_string() {
        let toml_str = r#"
            content_language = ""
        "#;
        let config: ChatConfig = toml::from_str(toml_str).expect("chat config should deserialize");
        assert_eq!(
            config.content_language,
            Some("".to_string()),
            "content_language should parse as Some(\"\") — handler layer filters empty to None"
        );
    }

    // Covers: Design 5.1 — missing content_language field defaults to None.
    // User Story: Query language aware rewrite — omitted field gives None (no language conversion).
    #[test]
    fn chat_config_missing_content_language_defaults_to_none() {
        let toml_str = r#"
            system_prompt = "test"
        "#;
        let config: ChatConfig = toml::from_str(toml_str).expect("chat config should deserialize");
        assert_eq!(
            config.content_language, None,
            "missing content_language should default to None"
        );
    }

    // Covers: Design 5.1 — missing [chat] section gives content_language = None via Default.
    // User Story: Query language aware rewrite — backward compatible, no [chat] means None.
    #[test]
    fn app_config_missing_chat_section_content_language_is_none() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"
            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]
        "#;
        let config: AppConfig = toml::from_str(toml_str).expect("config should deserialize");
        assert_eq!(
            config.chat.content_language, None,
            "missing [chat] section should give content_language = None"
        );
    }

    // --- RerankConfig tests (BE-D01) ---

    // Covers: Design 4.5.1 — RerankConfig default has enable=false, backward compatible.
    #[test]
    fn rerank_config_default_is_disabled() {
        let config = RerankConfig::default();
        assert!(!config.enable, "rerank should be disabled by default");
        assert_eq!(config.provider, RerankProviderType::OpenRouter);
        assert_eq!(config.top_n, 20);
        assert_eq!(config.timeout_secs, 3);
        assert!(config.model.is_none());
        assert!(config.api_key.is_none());
    }

    // Covers: Design 4.5.1 — RerankConfig parses all fields from TOML.
    #[test]
    fn rerank_config_parses_all_fields() {
        let toml_str = r#"
            enable = true
            provider = "big_model"
            model = "rerank-pro"
            top_n = 10
            timeout_secs = 5
            api_key = "sk-rerank-key"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("rerank config should deserialize");
        assert!(config.enable);
        assert_eq!(config.provider, RerankProviderType::BigModel);
        assert_eq!(config.model.as_deref(), Some("rerank-pro"));
        assert_eq!(config.top_n, 10);
        assert_eq!(config.timeout_secs, 5);
        assert_eq!(config.api_key.as_deref(), Some("sk-rerank-key"));
    }

    // Covers: BE Bailian — RerankConfig parses DashScope provider with default model.
    #[test]
    fn rerank_config_parses_dashscope_provider() {
        let toml_str = r#"
            enable = true
            provider = "dash_scope"
            model = "qwen3-rerank"
            top_n = 20
            timeout_secs = 3
            api_key = "sk-dashscope-key"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("rerank config should deserialize");
        assert!(config.enable);
        assert_eq!(config.provider, RerankProviderType::DashScope);
        assert_eq!(config.model.as_deref(), Some("qwen3-rerank"));
        assert_eq!(config.top_n, 20);
        assert_eq!(config.timeout_secs, 3);
        assert_eq!(config.api_key.as_deref(), Some("sk-dashscope-key"));
    }

    // Covers: Design 4.5.1 — Missing [rerank] section is backward compatible.
    #[test]
    fn rerank_config_missing_section_backward_compatible() {
        let toml_str = r#"
            [server]
            bind_address = "0.0.0.0:8080"
            log_level = "info"
            app_env = "development"
            enable_openapi = true

            [sqlite]
            path = "data/rwiki.db"

            [llm]
            api_key = "test"
            base_url = "https://example.com"
            model = "test-model"

            [embedding]

            [api]
        "#;
        let config: AppConfig =
            toml::from_str(toml_str).expect("config without [rerank] should deserialize");
        assert!(
            !config.rerank.enable,
            "missing [rerank] should default to disabled"
        );
    }

    // Covers: Design 4.5.1 — Invalid provider value causes deserialization error, not silent fallback.
    #[test]
    fn rerank_config_invalid_provider_fails() {
        let toml_str = r#"
            enable = true
            provider = "nonexistent"
        "#;
        let result = toml::from_str::<RerankConfig>(toml_str);
        assert!(
            result.is_err(),
            "invalid provider value should cause deserialization error"
        );
    }

    // Covers: Design 4.5.1 — Partial config uses defaults for missing fields.
    #[test]
    fn rerank_config_partial_uses_defaults() {
        let toml_str = r#"
            enable = true
            model = "custom-model"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("partial rerank config should deserialize");
        assert!(config.enable);
        assert_eq!(
            config.provider,
            RerankProviderType::OpenRouter,
            "missing provider should default to OpenRouter"
        );
        assert_eq!(config.model.as_deref(), Some("custom-model"));
        assert_eq!(config.top_n, 20, "missing top_n should default to 20");
        assert_eq!(
            config.timeout_secs, 3,
            "missing timeout_secs should default to 3"
        );
        assert!(
            config.api_key.is_none(),
            "missing api_key should default to None"
        );
        assert!(
            config.base_url.is_none(),
            "missing base_url should default to None"
        );
    }

    // Covers: DashScope 区域一致性 — base_url 字段可显式配置，默认 None（向后兼容）。
    #[test]
    fn rerank_config_parses_base_url_and_defaults_to_none() {
        // 默认 None
        assert_eq!(
            RerankConfig::default().base_url,
            None,
            "default base_url should be None (backward compatible)"
        );

        // 显式配置
        let toml_str = r#"
            enable = true
            provider = "dash_scope"
            base_url = "https://custom.example.com/reranks"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("rerank config with base_url should deserialize");
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://custom.example.com/reranks"),
            "base_url should parse explicit value"
        );
    }
}
