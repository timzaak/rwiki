//! MCP Server 工具模块：把知识库问答与检索能力以 MCP（Model Context
//! Protocol）只读工具的形式暴露给 Agent 客户端（Claude Code、Cursor 等）。
//!
//! 传输为 Streamable HTTP（`POST /mcp`，无状态会话），鉴权复用
//! `auth_middleware`（Bearer API Token + 可选 IP 白名单），由 `mcp_router`
//! 装配并按 `[mcp]` 配置段（section presence）条件挂载。
//!
//! 工具面仅两个只读工具，回答口径与公开聊天同源：复用 chat.rs 的
//! HTTP 无关管线函数，单轮语义（无历史、无摘要注入）。

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::tool;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use rwiki_core::config::ChannelValidationError;
use rwiki_core::infrastructure::vector_store::RetrievalScope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::application::http::middleware::auth_middleware;
use crate::application::http::state::AppState;

use super::chat::{build_preamble, format_context_xml, rewrite_query, search_and_rerank};

/// 检索工具 topK 上限（防 Agent 滥用；缺省 = retrieval.max_context_chunks）
const MCP_MAX_TOP_K: u32 = 20;

/// 工具执行期内部失败的统一对外文案；底层错误细节仅进 tracing。
const INTERNAL_ERROR_MESSAGE: &str = "知识库服务内部错误";

// ---------------------------------------------------------------------------
// 工具入参/返回结构（serde camelCase 对外；不纳入 OpenAPI）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct QaToolParams {
    /// Question text to answer from the channel's published knowledge base.
    query: String,
    /// Target channel identifier (must be configured in [channels]).
    channel_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct QaToolResult {
    answer: String,
    references: Vec<ReferenceItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceItem {
    /// 1-based 序号，对应回答内联引用 `[Source N]`
    index: usize,
    title: String,
    section: Option<String>,
    link: Option<String>,
    locale: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct SearchToolParams {
    /// Query text used for hybrid retrieval.
    query: String,
    /// Target channel identifier (must be configured in [channels]).
    channel_id: String,
    /// Max number of chunks to return (1..=20; defaults to retrieval config).
    top_k: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchToolResult {
    results: Vec<SearchChunkItem>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchChunkItem {
    title: String,
    section: Option<String>,
    content: String,
    link: Option<String>,
    locale: Option<String>,
    score: f64,
    document_id: String,
}

// ---------------------------------------------------------------------------
// ServerHandler 与工具定义
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct McpTools {
    state: Arc<AppState>,
}

#[tool_router]
impl McpTools {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    #[tool(
        description = "Answer a question from the published knowledge base of a RWiki channel; returns a complete answer with source references"
    )]
    async fn rwiki_qa(
        &self,
        Parameters(params): Parameters<QaToolParams>,
    ) -> Result<CallToolResult, McpError> {
        execute_tool(
            "rwiki_qa",
            &params.channel_id,
            run_qa(&self.state, &params),
            |result| result.references.len(),
        )
        .await
    }

    #[tool(
        description = "Search raw knowledge chunks of a RWiki channel; returns relevance-sorted snippets with source info (published documents only)"
    )]
    async fn rwiki_search(
        &self,
        Parameters(params): Parameters<SearchToolParams>,
    ) -> Result<CallToolResult, McpError> {
        execute_tool(
            "rwiki_search",
            &params.channel_id,
            run_search(&self.state, &params),
            |result| result.results.len(),
        )
        .await
    }
}

#[tool_handler]
impl ServerHandler for McpTools {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("rwiki", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "RWiki knowledge base. Both tools require a configured channelId; \
                 answers cite sources as [Source N] mapping to references[]."
                    .to_string(),
            )
    }
}

/// 把工具执行结果转成 MCP 返回：成功与业务失败均为 `content[0].text` 的
/// JSON/文案（业务失败经 `isError` 返回，让 Agent 能读到原因自行纠正）。
fn finish<T: Serialize>(outcome: Result<T, String>) -> Result<CallToolResult, McpError> {
    match outcome {
        Ok(payload) => {
            let text = serde_json::to_string(&payload).map_err(|e| {
                tracing::error!("mcp tool result serialization failed: {e}");
                McpError::internal_error(INTERNAL_ERROR_MESSAGE, None)
            })?;
            Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
        }
        Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(message)])),
    }
}

/// 工具方法公共样板：计时 → 执行 → 命中计数 → 统一结束日志 → 组装返回。
async fn execute_tool<T: Serialize>(
    tool: &'static str,
    channel_id: &str,
    run: impl std::future::Future<Output = Result<T, String>>,
    count: impl Fn(&T) -> usize,
) -> Result<CallToolResult, McpError> {
    let start = Instant::now();
    let outcome = run.await;
    let matched = outcome.as_ref().map_or(0, count);
    tracing::info!(
        tool,
        channel = %channel_id,
        matched,
        elapsed_ms = start.elapsed().as_millis() as u64,
        "mcp tool call finished"
    );
    finish(outcome)
}

// ---------------------------------------------------------------------------
// 工具执行流程
// ---------------------------------------------------------------------------

/// 校验 channelId：trim、非空、已配置；错误文案与公开聊天逐字一致。
fn validate_channel<'a>(state: &'a AppState, channel_id: &'a str) -> Result<&'a str, String> {
    state
        .channels_config
        .require_configured(channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => "channelId 不能为空".to_string(),
            ChannelValidationError::NotConfigured(id) => format!("频道 {id} 未配置"),
        })
}

/// 工具入口公共校验：query trim 非空 + channelId 已配置。
fn validate_query_and_channel<'a>(
    state: &'a AppState,
    query: &'a str,
    channel_id: &'a str,
) -> Result<(&'a str, &'a str), String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("查询不能为空".to_string());
    }
    let channel_id = validate_channel(state, channel_id)?;
    Ok((query, channel_id))
}

/// 单轮改写：无历史（history = []）+ content_language 过滤。
async fn rewrite_single_turn(state: &AppState, query: &str) -> Vec<String> {
    let content_language = state
        .chat_config
        .content_language
        .as_deref()
        .filter(|s| !s.is_empty());
    rewrite_query(
        &state.llm_client,
        &state.llm_model,
        query,
        &[],
        content_language,
        &state.metrics,
    )
    .await
}

/// 底层失败的统一收敛：细节进 tracing，对外只返回固定文案。
fn internal_error(stage: &str, err: impl std::fmt::Display) -> String {
    tracing::error!("mcp tool internal failure at {stage}: {err}");
    INTERNAL_ERROR_MESSAGE.to_string()
}

/// 问答流程：校验 → 频道有已发布文档检查 → 改写 → 检索+rerank →
/// 上下文组装 → 非流式生成 → `{ answer, references }`（单轮：无历史、无摘要）。
async fn run_qa(state: &AppState, params: &QaToolParams) -> Result<QaToolResult, String> {
    let (query, channel_id) = validate_query_and_channel(state, &params.query, &params.channel_id)?;

    let channels = [channel_id.to_string()];
    if !state
        .vector_store
        .has_published_documents_for_channels(&channels)
        .await
    {
        return Err("当前频道没有可用文档".to_string());
    }

    let rewritten_queries = rewrite_single_turn(state, query).await;

    // 检索参数与公开聊天同口径（chat_inner 先例）
    let scope = RetrievalScope::Channel(channel_id.to_string());
    let results = search_and_rerank(
        &state.vector_store,
        &state.reranker,
        &state.rerank_config,
        query,
        &rewritten_queries,
        state.retrieval_config.search_top_k_per_query.max(1),
        state.retrieval_config.max_context_chunks.max(1),
        &state.metrics,
        &scope,
    )
    .await
    .map_err(|e| internal_error("retrieval", e))?;

    // 单轮语义：summary = None；频道级系统提示词优先、回退全局
    let preamble = build_preamble(
        state
            .channels_config
            .resolved_system_prompt(Some(channel_id), &state.chat_config.system_prompt),
        None,
        &format_context_xml(&results),
    );

    // 非流式完整回答（eval 先例）
    let agent = state
        .llm_client
        .agent(&state.llm_model)
        .preamble(&preamble)
        .build();
    let answer = agent
        .prompt(query)
        .await
        .map_err(|e| internal_error("llm generation", e))?;

    let references = results
        .iter()
        .enumerate()
        .map(|(i, r)| ReferenceItem {
            index: i + 1,
            title: r.title.clone(),
            section: r.section.clone(),
            link: r.link.clone(),
            locale: r.locale.clone(),
        })
        .collect();

    Ok(QaToolResult { answer, references })
}

/// 检索流程：校验 → 改写 → 检索+rerank → `{ results }`。
/// 不做「频道无已发布文档」预检查：无文档与无命中统一返回空列表。
async fn run_search(
    state: &AppState,
    params: &SearchToolParams,
) -> Result<SearchToolResult, String> {
    let (query, channel_id) = validate_query_and_channel(state, &params.query, &params.channel_id)?;

    let top_k = params
        .top_k
        .unwrap_or(state.retrieval_config.max_context_chunks as u32);
    if top_k == 0 || top_k > MCP_MAX_TOP_K {
        return Err(format!("topK 必须在 1 到 {MCP_MAX_TOP_K} 之间"));
    }
    let top_k = top_k as usize;

    let rewritten_queries = rewrite_single_turn(state, query).await;

    let scope = RetrievalScope::Channel(channel_id.to_string());
    let results = search_and_rerank(
        &state.vector_store,
        &state.reranker,
        &state.rerank_config,
        query,
        &rewritten_queries,
        top_k,
        top_k,
        &state.metrics,
        &scope,
    )
    .await
    .map_err(|e| internal_error("retrieval", e))?;

    let results = results
        .into_iter()
        .map(|r| SearchChunkItem {
            title: r.title,
            section: r.section,
            content: r.content,
            link: r.link,
            locale: r.locale,
            score: r.score,
            document_id: r.document_id,
        })
        .collect();

    Ok(SearchToolResult { results })
}

// ---------------------------------------------------------------------------
// 路由装配
// ---------------------------------------------------------------------------

/// 构建 /mcp 子路由：无状态 Streamable HTTP + auth_middleware（与 doc_router 同构）。
///
/// 无状态会话（`NeverSessionManager`）：服务端不下发 `Mcp-Session-Id`、
/// 不保留任何 MCP 调用间状态；`GET /mcp`（服务端推送流）返回 405。
pub fn mcp_router(state: Arc<AppState>) -> Router {
    let config = StreamableHttpServerConfig::default()
        // 单轮语义要求对**所有**协议版本无状态：默认
        // `legacy_session_mode: true` 会在 initialize 时创建会话，
        // 被 NeverSessionManager 拒绝（"Session management is not supported"）。
        .with_legacy_session_mode(false)
        // 单请求-响应优先回 application/json（更通用的客户端兼容形态），
        // 需要流式时自动回退 text/event-stream。
        .with_json_response(true)
        // 远程 MCP 客户端以部署方任意主机名接入；端点已由 auth_middleware
        // （Bearer Token + 可选 IP 白名单）保护，rmcp 默认的 loopback-only
        // Host 白名单（面向本地浏览器服务的 DNS rebinding 防护）会拒绝合法
        // 远程客户端，故关闭。
        .disable_allowed_hosts();

    let handler_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(McpTools::new(handler_state.clone())),
        NeverSessionManager::default().into(),
        config,
    );
    Router::new()
        .nest_service("/mcp", service)
        .layer(axum::middleware::from_fn_with_state(state, auth_middleware))
}
