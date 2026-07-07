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
    /// Rerank 精排配置；省略整个 `[rerank]` 段时为 `None`，即关闭 rerank。
    /// 存在 `[rerank]` 段（即使为空）即视为启用。
    #[serde(default)]
    pub rerank: Option<RerankConfig>,
    /// 低相关召回记录配置；省略 `[low_recall]` 段 → None（关闭）。
    /// 存在段即启用，参照 rerank 惯例。
    #[serde(default)]
    pub low_recall: Option<LowRecallConfig>,
    /// 多频道配置；省略 `[channels]` 段 → 空频道列表。
    /// 系统启动时校验至少存在一个已配置频道。
    #[serde(default)]
    pub channels: ChannelsConfig,
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
        if let Ok(ranges) = env::var("API_ALLOWED_IP_RANGES") {
            config.api.allowed_ip_ranges = parse_env_list(&ranges);
        }
        if let Ok(origins) = env::var("CORS_ALLOWED_ORIGINS") {
            config.server.allowed_origins = parse_env_list(&origins);
        }
        if let Ok(key) = env::var("OTEL_LICENSE_KEY") {
            config.otel.license_key = key;
        }
        if let Ok(key) = env::var("RERANK_API_KEY") {
            // RERANK_API_KEY only applies when [rerank] is present; a missing
            // section means rerank is disabled, so there is nothing to override.
            if let Some(rerank) = config.rerank.as_mut() {
                rerank.api_key = Some(key);
            }
        }
        Ok(config)
    }
}

fn parse_env_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
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
    /// CORS 允许的来源（供第三方网站嵌入聊天 widget 调用 /api/chat 等）。
    /// 空 = 允许所有来源（默认，向后兼容，等同 `Access-Control-Allow-Origin: *`）；
    /// 非空 = 仅允许列表中的精确 origin，用于生产环境收紧、防止接口被未授权站点滥用。
    /// 也可经环境变量 `CORS_ALLOWED_ORIGINS`（逗号分隔）覆盖。
    #[serde(default)]
    pub allowed_origins: Vec<String>,
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
    #[serde(default)]
    pub allowed_ip_ranges: Vec<String>,
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
///
/// 是否启用 rerank 不再由字段控制，而是由配置中是否存在 `[rerank]` 段决定：
/// - 缺失 `[rerank]` 段 → 关闭（`AppConfig.rerank` 为 `None`）
/// - 存在 `[rerank]` 段 → 启用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
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
            provider: RerankProviderType::default(),
            model: None,
            top_n: default_top_n(),
            timeout_secs: default_timeout_secs(),
            api_key: None,
            base_url: None,
        }
    }
}

fn default_low_recall_threshold() -> f64 {
    0.3 // 待首批记录校准（rerank 分数分布未知）
}

/// 低相关召回记录配置；省略整个 `[low_recall]` 段时为 `None`（关闭）。
/// 存在段（即使为空）即视为启用（参照 rerank `[rerank]` section presence 惯例）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowRecallConfig {
    /// top-1 相关度分数低于此值时记录（0..1，rerank relevance_score 语义）。
    /// 默认 0.3，待首批记录校准。
    #[serde(default = "default_low_recall_threshold")]
    pub threshold: f64,
}

impl Default for LowRecallConfig {
    fn default() -> Self {
        Self {
            threshold: default_low_recall_threshold(),
        }
    }
}

/// 单个频道配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// 人类可读频道名称（用于管理后台下拉框、频道列表）。
    pub name: String,
    /// 频道级系统提示词；省略时使用全局 `[chat].system_prompt`。
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// 频道级推荐问题；省略时该频道返回空数组。
    /// Key 为语言标签（如 "default"、"zh-CN"、"en"），Value 为该语言的推荐问题列表。
    #[serde(default)]
    pub suggested_questions: Option<HashMap<String, Vec<String>>>,
}

/// 多频道配置集合。
///
/// TOML 中以 `[channels.<channelId>]` 形式定义，透传为内部 `HashMap`。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelsConfig {
    /// 频道 ID 到频道配置的映射。
    pub channels: HashMap<String, ChannelConfig>,
}

/// 对外暴露的频道元数据（ID 与显示名称）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMetadata {
    pub id: String,
    pub name: String,
}

/// 频道 ID 校验错误。
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ChannelValidationError {
    #[error("channelId cannot be empty")]
    Empty,
    #[error("channel {0} is not configured")]
    NotConfigured(String),
}

impl ChannelsConfig {
    /// 查找已配置频道。
    pub fn get(&self, channel_id: &str) -> Option<&ChannelConfig> {
        self.channels.get(channel_id)
    }

    /// 是否未配置任何频道。
    pub fn is_empty(&self) -> bool {
        self.channels.is_empty()
    }

    /// 返回所有已配置频道的公开元数据列表（仅 ID 与名称）。
    pub fn list_metadata(&self) -> Vec<ChannelMetadata> {
        self.channels
            .iter()
            .map(|(id, cfg)| ChannelMetadata {
                id: id.clone(),
                name: cfg.name.clone(),
            })
            .collect()
    }

    /// 去除首尾空白并校验 channel_id 非空。
    pub fn normalize_channel_id(channel_id: &str) -> Option<&str> {
        let trimmed = channel_id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    /// 校验并返回去除空白后的 channel_id，同时确认该频道已配置。
    pub fn require_configured<'a>(
        &self,
        channel_id: &'a str,
    ) -> Result<&'a str, ChannelValidationError> {
        let normalized =
            Self::normalize_channel_id(channel_id).ok_or(ChannelValidationError::Empty)?;
        if self.channels.contains_key(normalized) {
            Ok(normalized)
        } else {
            Err(ChannelValidationError::NotConfigured(
                normalized.to_string(),
            ))
        }
    }

    /// 批量校验多个 channelId：逐个 trim/去空/校验已配置，返回**去重并按字典序排序**的 Vec。
    ///
    /// 返回值保证：元素唯一、非空、均已配置、且顺序稳定（字典序）。
    /// 稳定的顺序用于派生 session_key / 低召回记录 channel_id 等存储键，使同一组频道
    /// 不论输入顺序如何都映射到同一作用域与存储桶，避免跨频道组合串话。
    ///
    /// 语义：
    /// - 输入为空切片或全为空白字符串 → `ChannelValidationError::Empty`
    /// - 任一频道未配置 → 返回首个未配置频道的 `NotConfigured(id)`
    pub fn require_all_configured(
        &self,
        channel_ids: &[String],
    ) -> Result<Vec<String>, ChannelValidationError> {
        let mut seen = std::collections::HashSet::new();
        let mut normalized: Vec<String> = Vec::new();
        for id in channel_ids {
            let trimmed = Self::normalize_channel_id(id).ok_or(ChannelValidationError::Empty)?;
            if !seen.contains(trimmed) {
                if !self.channels.contains_key(trimmed) {
                    return Err(ChannelValidationError::NotConfigured(trimmed.to_string()));
                }
                seen.insert(trimmed.to_string());
                normalized.push(trimmed.to_string());
            }
        }
        if normalized.is_empty() {
            return Err(ChannelValidationError::Empty);
        }
        normalized.sort();
        Ok(normalized)
    }

    /// 解析频道系统提示词：频道级配置存在且非空时优先，否则回退到全局提示词。
    pub fn resolved_system_prompt<'a>(
        &'a self,
        channel_id: Option<&str>,
        global: &'a str,
    ) -> &'a str {
        channel_id
            .and_then(|id| self.channels.get(id))
            .and_then(|s| s.system_prompt.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or(global)
    }

    /// 多频道版本的系统提示词解析：取频道列表中**首个**已配置且非空的频道级提示词，
    /// 否则回退到全局提示词。
    ///
    /// 调用方通常传入已排序（`require_all_configured` 返回值）的频道列表，
    /// 因此"首个"是确定的（字典序最小的频道）。多频道场景下无法为每个频道分别拼装
    /// 单一 system prompt，取首个是最简单、可预测的策略。
    pub fn resolved_system_prompt_multi<'a>(
        &'a self,
        channel_ids: Option<&[String]>,
        global: &'a str,
    ) -> &'a str {
        channel_ids
            .and_then(|ids| ids.iter().find_map(|id| self.channels.get(id)))
            .and_then(|s| s.system_prompt.as_deref())
            .filter(|s| !s.is_empty())
            .unwrap_or(global)
    }
}

/// 启动校验：拒绝存在 `channel_id IS NULL` 历史行的表。
///
/// `tables` 传入需要检查的表名；后续 item 可扩展此列表而不新增校验入口点。
/// 若表尚未包含 `channel_id` 列（例如 migration 尚未执行），则跳过该校验，
/// 避免在逐步演进的 schema 上误报。
///
/// 注意：表名来自代码内部常量，不要传入用户输入。
pub async fn validate_historical_rows_have_channel_id(
    sqlite: &tokio_rusqlite::Connection,
    tables: &[&str],
) -> Result<(), anyhow::Error> {
    for table in tables {
        let table_name = table.to_string();
        let table_name_for_info = table_name.clone();
        let has_channel_id: bool = sqlite
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = 'channel_id'",
                )?;
                let count: i64 = stmt.query_row([&table_name_for_info], |row| row.get(0))?;
                Ok::<_, rusqlite::Error>(count > 0)
            })
            .await?;

        if !has_channel_id {
            continue;
        }

        let count: i64 = sqlite
            .call(move |conn| {
                let sql = format!("SELECT COUNT(*) FROM {table_name} WHERE channel_id IS NULL");
                conn.query_row(&sql, [], |row| row.get(0))
            })
            .await?;
        if count > 0 {
            return Err(anyhow::anyhow!(
                "Found {count} historical row(s) in `{table}` with missing channel_id. \
                 Please backfill channel_id before starting the service."
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "config_scenarios.rs"]
mod config_scenarios;

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn take_env(var: &str) -> Option<String> {
        match env::var(var) {
            Ok(val) => {
                env::remove_var(var);
                Some(val)
            }
            Err(_) => None,
        }
    }

    fn restore_env(var: &str, prev: Option<String>) {
        match prev {
            Some(val) => env::set_var(var, val),
            None => env::remove_var(var),
        }
    }

    fn write_api_config(api_body: &str) -> NamedTempFile {
        let content = format!(
            r#"
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
            {api_body}
        "#
        );
        let mut file = NamedTempFile::new().expect("should create temp file");
        file.write_all(content.as_bytes())
            .expect("should write config content");
        file
    }

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
    fn api_token_config_parses_ip_ranges() {
        let toml_str = r#"
            token = "test-token"
            allowed_ip_ranges = ["203.0.113.0/24", "2001:db8::/32"]
        "#;
        let config: ApiTokenConfig =
            toml::from_str(toml_str).expect("api config should deserialize");
        assert_eq!(
            config.allowed_ip_ranges,
            vec!["203.0.113.0/24".to_string(), "2001:db8::/32".to_string()],
            "allowed_ip_ranges should preserve configured CIDR strings for startup parsing"
        );
    }

    #[test]
    fn api_ip_ranges_env_vars_override_file_values() {
        let prev_token = take_env("API_TOKEN");
        let prev_allowed = take_env("API_ALLOWED_IP_RANGES");

        env::set_var("API_ALLOWED_IP_RANGES", "203.0.113.0/24, 2001:db8::/32");

        let file = write_api_config(
            r#"
            token = "file-token"
            allowed_ip_ranges = ["198.51.100.0/24"]
            "#,
        );
        let config = AppConfig::load(file.path()).expect("config should load");

        assert_eq!(
            config.api.allowed_ip_ranges,
            vec!["203.0.113.0/24".to_string(), "2001:db8::/32".to_string()],
            "API_ALLOWED_IP_RANGES must replace the file allow list"
        );

        restore_env("API_TOKEN", prev_token);
        restore_env("API_ALLOWED_IP_RANGES", prev_allowed);
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

    // Covers: RerankConfig defaults — provider/top_n/timeout_secs/model/api_key/base_url.
    #[test]
    fn rerank_config_defaults() {
        let config = RerankConfig::default();
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
            provider = "big_model"
            model = "rerank-pro"
            top_n = 10
            timeout_secs = 5
            api_key = "sk-rerank-key"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("rerank config should deserialize");
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
            provider = "dash_scope"
            model = "qwen3-rerank"
            top_n = 20
            timeout_secs = 3
            api_key = "sk-dashscope-key"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("rerank config should deserialize");
        assert_eq!(config.provider, RerankProviderType::DashScope);
        assert_eq!(config.model.as_deref(), Some("qwen3-rerank"));
        assert_eq!(config.top_n, 20);
        assert_eq!(config.timeout_secs, 3);
        assert_eq!(config.api_key.as_deref(), Some("sk-dashscope-key"));
    }

    // Covers: Enablement is decided by section presence — a missing `[rerank]`
    // section deserializes to `None` (disabled). This is the load-bearing
    // semantic: rerank is OFF unless the section exists.
    #[test]
    fn rerank_disabled_when_section_absent() {
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
            config.rerank.is_none(),
            "missing [rerank] section should mean rerank is disabled"
        );
    }

    // Covers: Enablement is decided by section presence — a present `[rerank]`
    // section (even with only defaults) deserializes to `Some` (enabled).
    #[test]
    fn rerank_enabled_when_section_present() {
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

            [rerank]

            [api]
        "#;
        let config: AppConfig =
            toml::from_str(toml_str).expect("config with [rerank] should deserialize");
        assert!(
            config.rerank.is_some(),
            "present [rerank] section should mean rerank is enabled"
        );
    }

    // Covers: Design 4.5.1 — Invalid provider value causes deserialization error, not silent fallback.
    #[test]
    fn rerank_config_invalid_provider_fails() {
        let toml_str = r#"
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
            model = "custom-model"
        "#;
        let config: RerankConfig =
            toml::from_str(toml_str).expect("partial rerank config should deserialize");
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

    // --- ChannelConfig / ChannelsConfig tests (BE-D01) ---

    // Covers: Design 5.1 — missing [channels] section defaults to empty.
    #[test]
    fn channels_config_defaults_to_empty() {
        let config = ChannelsConfig::default();
        assert!(
            config.is_empty(),
            "default ChannelsConfig should have no channels"
        );
        assert!(config.list_metadata().is_empty());
    }

    // Covers: Design 5.1 — [channels.<id>] parses name, system_prompt and suggested_questions.
    #[test]
    fn channels_config_parses_from_toml() {
        // When parsed standalone, ChannelsConfig is transparent and expects
        // top-level keys to be channel ids.
        let toml_str = r#"
            [help_center]
            name = "Help Center"
            system_prompt = "You are Help Center."
            [help_center.suggested_questions]
            default = ["How do I start?"]
            zh-CN = ["如何快速上手"]

            [dev_docs]
            name = "Developer Docs"
        "#;
        let config: ChannelsConfig =
            toml::from_str(toml_str).expect("channels config should parse");
        assert_eq!(config.channels.len(), 2);

        let help = config.get("help_center").expect("help_center should exist");
        assert_eq!(help.name, "Help Center");
        assert_eq!(help.system_prompt.as_deref(), Some("You are Help Center."));
        let questions = help.suggested_questions.as_ref().unwrap();
        assert_eq!(questions.get("default").unwrap(), &vec!["How do I start?"]);
        assert_eq!(questions.get("zh-CN").unwrap(), &vec!["如何快速上手"]);

        let dev = config.get("dev_docs").expect("dev_docs should exist");
        assert_eq!(dev.name, "Developer Docs");
        assert!(dev.system_prompt.is_none());
        assert!(dev.suggested_questions.is_none());
    }

    // Covers: Design 5.1 — list_metadata returns only id + name in deterministic order.
    #[test]
    fn channels_config_list_metadata_includes_id_and_name() {
        let mut channels = ChannelsConfig::default();
        channels.channels.insert(
            "b".to_string(),
            ChannelConfig {
                name: "B Channel".to_string(),
                system_prompt: None,
                suggested_questions: None,
            },
        );
        channels.channels.insert(
            "a".to_string(),
            ChannelConfig {
                name: "A Channel".to_string(),
                system_prompt: None,
                suggested_questions: None,
            },
        );
        let metadata = channels.list_metadata();
        assert_eq!(metadata.len(), 2);
        // HashMap iteration order is not guaranteed; sort by id for assertion.
        let mut sorted = metadata;
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(sorted[0].id, "a");
        assert_eq!(sorted[0].name, "A Channel");
        assert_eq!(sorted[1].id, "b");
        assert_eq!(sorted[1].name, "B Channel");
    }

    // Covers: BE-D01 helper — empty/whitespace channel ids are rejected.
    #[test]
    fn normalize_channel_id_trims_and_rejects_empty() {
        assert_eq!(
            ChannelsConfig::normalize_channel_id("  help  "),
            Some("help")
        );
        assert_eq!(ChannelsConfig::normalize_channel_id("help"), Some("help"));
        assert_eq!(ChannelsConfig::normalize_channel_id(""), None);
        assert_eq!(ChannelsConfig::normalize_channel_id("   "), None);
    }

    // Covers: BE-D01 helper — require_configured enforces non-empty and configured channel.
    #[test]
    fn require_configured_rejects_empty_and_unknown() {
        let mut channels = ChannelsConfig::default();
        channels.channels.insert(
            "help".to_string(),
            ChannelConfig {
                name: "Help".to_string(),
                system_prompt: None,
                suggested_questions: None,
            },
        );

        assert_eq!(
            channels.require_configured("help"),
            Ok("help"),
            "configured channel should resolve"
        );
        assert_eq!(
            channels.require_configured("  help  "),
            Ok("help"),
            "whitespace should be normalized"
        );
        assert_eq!(
            channels.require_configured(""),
            Err(ChannelValidationError::Empty),
            "empty channel id should fail"
        );
        assert_eq!(
            channels.require_configured("unknown"),
            Err(ChannelValidationError::NotConfigured("unknown".to_string())),
            "unknown channel should fail"
        );
    }

    // Covers: Design 5.1 — resolved_system_prompt falls back to global when channel missing
    // or channel-level prompt is empty/unset.
    #[test]
    fn resolved_system_prompt_uses_channel_or_global_fallback() {
        let mut channels = ChannelsConfig::default();
        channels.channels.insert(
            "help".to_string(),
            ChannelConfig {
                name: "Help".to_string(),
                system_prompt: Some("Channel prompt.".to_string()),
                suggested_questions: None,
            },
        );
        channels.channels.insert(
            "empty".to_string(),
            ChannelConfig {
                name: "Empty".to_string(),
                system_prompt: Some("".to_string()),
                suggested_questions: None,
            },
        );

        let global = "Global prompt.";
        assert_eq!(
            channels.resolved_system_prompt(Some("help"), global),
            "Channel prompt."
        );
        assert_eq!(
            channels.resolved_system_prompt(Some("empty"), global),
            global
        );
        assert_eq!(
            channels.resolved_system_prompt(Some("missing"), global),
            global
        );
        assert_eq!(channels.resolved_system_prompt(None, global), global);
    }

    // Covers: BE-D01 — [channels.<id>] section parses within full AppConfig.
    #[test]
    fn app_config_parses_channels_section() {
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

            [channels.help_center]
            name = "Help Center"
            system_prompt = "Channel prompt."
            [channels.help_center.suggested_questions]
            default = ["如何快速上手"]

            [channels.dev_docs]
            name = "Developer Docs"
        "#;
        let config: AppConfig =
            toml::from_str(toml_str).expect("config with channels should parse");
        assert_eq!(config.channels.channels.len(), 2);
        assert_eq!(
            config.channels.get("help_center").unwrap().name,
            "Help Center"
        );
        assert_eq!(config.channels.get("dev_docs").unwrap().system_prompt, None);
    }

    // Covers: BE-D01 startup validation — tolerate tables without channel_id column,
    // and fail loudly when channel_id IS NULL rows exist after the column is added.
    #[tokio::test]
    async fn validate_historical_rows_detects_null_channel_id() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
        let sqlite = tokio_rusqlite::Connection::from(conn);

        // Create a documents table without channel_id column; validation should pass.
        sqlite
            .call(|conn| {
                conn.execute(
                    "CREATE TABLE documents (id TEXT PRIMARY KEY, file_name TEXT NOT NULL)",
                    [],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("create documents table");
        validate_historical_rows_have_channel_id(&sqlite, &["documents"])
            .await
            .expect("validation should pass when channel_id column is absent");

        // Add channel_id column and a NULL row; validation should now fail.
        sqlite
            .call(|conn| {
                conn.execute("ALTER TABLE documents ADD COLUMN channel_id TEXT", [])?;
                conn.execute(
                    "INSERT INTO documents (id, file_name) VALUES (?1, ?2)",
                    rusqlite::params!["doc-1", "test.txt"],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("seed null channel_id row");

        let err = validate_historical_rows_have_channel_id(&sqlite, &["documents"])
            .await
            .expect_err("validation should fail with null channel_id rows");
        assert!(
            err.to_string().contains("missing channel_id"),
            "error should mention missing channel_id: {err}"
        );

        // Backfill and re-validate; should pass again.
        sqlite
            .call(|conn| {
                conn.execute(
                    "UPDATE documents SET channel_id = 'help_center' WHERE channel_id IS NULL",
                    [],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("backfill channel_id");
        validate_historical_rows_have_channel_id(&sqlite, &["documents"])
            .await
            .expect("validation should pass after backfill");
    }
}
