use rig::providers::openai;
use rwiki_core::config::ChatConfig;
use rwiki_core::domain::chat::ChatSession;
use rwiki_core::infrastructure::vector_store::VectorStoreManager;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 应用共享状态
///
/// 所有路由处理函数通过 Axum 的 State 提取器访问此结构体。
/// 使用 Arc 包装以实现高效的共享所有权。
#[derive(Clone)]
pub struct AppState {
    /// SQLite 连接 — 用于文档元数据和健康检查
    pub sqlite: Arc<tokio_rusqlite::Connection>,
    /// 是否启用 OpenAPI/Swagger 文档
    pub enable_openapi: bool,
    /// 向量存储管理器 — 用于文档分块的向量检索
    pub vector_store: Arc<VectorStoreManager>,
    /// 聊天会话存储 — 内存中的会话管理（tokio::sync::Mutex 支持 .await 跨锁持有）
    pub chat_sessions: Arc<Mutex<HashMap<String, ChatSession>>>,
    /// LLM 客户端（OpenAI 兼容 Completions）
    pub llm_client: openai::CompletionsClient,
    /// LLM model name for building agents
    pub llm_model: String,
    /// API Token for Bearer Token authentication
    pub api_token: String,
    /// 聊天配置（系统提示词、滑动窗口、压缩阈值等）
    pub chat_config: ChatConfig,
    /// 静态文件目录路径（含 widget JS 等），为 None 时不托管
    pub static_dir: Option<String>,
}
