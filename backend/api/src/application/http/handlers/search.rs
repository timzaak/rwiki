//! 公开频道级知识片段搜索：`POST /api/search`。
//!
//! 按已配置频道检索已发布文档的知识片段，返回按相关性排序（page 级，
//! 页内保持文档顺序）的带来源信息结果列表，不经 LLM 生成回答。检索口径
//! 由 `[retrieval].public_search_mode` 选择：
//! - `fts`（默认）：延迟优先的纯 FTS 关键词口径（`search_fts`：无查询
//!   改写、无向量化、无 rerank、无窗口扩展，全程无网络调用）。召回特性：
//!   无词面重叠的语义查询返回空结果；仅标题命中不召回（标题只影响排序
//!   加权）。
//! - `hybrid`：与 MCP `rwiki_search` 同一混合检索口径（单轮改写 →
//!   `search_and_rerank`），恢复语义召回，代价是每次请求的改写与向量化
//!   网络往返。
//!
//! 入口校验与 topK 口径两种模式均复用 `handlers/mcp.rs` 的共享 helper。
//! 公开端点（无凭据要求，携带凭据照常处理，与 `/api/chat` 同一暴露
//! 级别），错误细节不外露（500 固定文案）。

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use utoipa::ToSchema;

use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use rwiki_core::config::PublicSearchMode;
use rwiki_core::domain::search::{search_and_rerank, search_fts};
use rwiki_core::infrastructure::markdown_parser::markdown_to_plain_text;
use rwiki_core::infrastructure::vector_store::{RetrievalScope, SearchResult};

use super::mcp::{
    resolve_top_k, rewrite_single_turn, validate_query_and_channel, INTERNAL_ERROR_MESSAGE,
};

#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchRequest {
    /// Query text used for hybrid retrieval.
    /// Missing is treated as empty and rejected with 400 by the handler.
    #[serde(default)]
    #[schema(required = true)]
    pub query: String,
    /// Target channel identifier (must be configured in [channels]).
    /// Missing is treated as empty and rejected with 400 by the handler.
    #[serde(rename = "channelId", default)]
    #[schema(required = true)]
    pub channel_id: String,
    /// Max number of chunks to return (1..=20). Defaults to the retrieval
    /// config, capped to 1..=20. Integer values outside this range (including
    /// negatives) are rejected with 400; non-integer garbage (floats, strings)
    /// is rejected by the framework with 422.
    #[serde(rename = "topK", default)]
    pub top_k: Option<i64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResponse {
    /// Chunks ordered by relevance at page granularity: pages by their best
    /// chunk score (descending), chunks within a page in document order
    /// (sub_index). Render in the returned order; scores are not necessarily
    /// monotonic along the list. Empty only when retrieval has zero hits
    /// (e.g. the channel has no published documents); this is not an error.
    /// No relevance threshold is applied.
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub title: String,
    pub section: Option<String>,
    /// Plain-text rendering of the chunk for display snippets: markdown
    /// syntax stripped, block boundaries collapsed to single spaces. The
    /// stored chunk and the MCP/chat retrieval paths keep the raw markdown.
    pub content: String,
    pub link: Option<String>,
    pub locale: Option<String>,
    /// Relevance score of this chunk (higher = more relevant; includes the
    /// title-match bonus when its page title matches). Scores are NOT
    /// comparable across pipelines or configurations.
    pub score: f64,
    pub document_id: String,
}

impl From<SearchResult> for SearchResultItem {
    fn from(r: SearchResult) -> Self {
        Self {
            title: r.title,
            section: r.section,
            content: markdown_to_plain_text(&r.content),
            link: r.link,
            locale: r.locale,
            score: r.score,
            document_id: r.document_id,
        }
    }
}

/// Public channel-scoped knowledge chunk search (no authentication).
#[utoipa::path(
    post,
    path = "/api/search",
    tag = "search",
    request_body = SearchRequest,
    responses(
        (status = 200, description = "Knowledge chunks ordered by page relevance (empty on zero hits)",
         body = SearchResponse),
        (status = 400, description = "Invalid query, channelId, or topK",
         body = ErrorResponse),
        (status = 422, description = "Malformed JSON body or non-integer topK (framework rejection, plain-text body)"),
        (status = 500, description = "Internal retrieval failure (generic message)",
         body = ErrorResponse)
    )
)]
pub async fn search_chunks(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, ApiError> {
    // 计时与 MCP execute_tool 同口径：从进入 handler 起算（含校验）。
    let start = Instant::now();
    let (query, channel_id) = validate_query_and_channel(&state, &req.query, &req.channel_id)
        .map_err(ApiError::bad_request)?;

    let top_k = resolve_top_k(req.top_k, state.retrieval_config.max_context_chunks)
        .map_err(ApiError::bad_request)?;

    let scope = RetrievalScope::Channel(channel_id.to_string());
    let retrieval = match state.retrieval_config.public_search_mode {
        // 纯 FTS 关键词检索：空库/频道无文档/零命中统一空结果（FTS 空索引
        // 优雅降级为空），无改写与向量化网络调用。
        PublicSearchMode::Fts => {
            search_fts(&state.vector_store, query, top_k, &state.metrics, &scope).await
        }
        PublicSearchMode::Hybrid => {
            // 全局空库（尚无任何已索引 chunk）时检索原语会报错；频道无文档与
            // 检索零命中统一返回空结果而非错误，故短路为空列表并省掉注定无用的
            // 改写 + embedding 调用（空结果落入下方公共 finish 日志）。预检查
            // 自身失败按内部错误处理（500 固定文案），不当作「空库」静默放行。
            if state.vector_store.is_empty().await.map_err(|e| {
                tracing::error!(error = %e, channel = %channel_id, "empty-KB precheck failed");
                ApiError::internal(INTERNAL_ERROR_MESSAGE)
            })? {
                Ok(Vec::new())
            } else {
                // 单轮语义：无历史（与 MCP 检索工具共用 rewrite_single_turn），
                // content_language 频道级优先、回退 chat 全局配置。
                let search_queries = rewrite_single_turn(&state, channel_id, query).await;

                search_and_rerank(
                    &state.vector_store,
                    &state.reranker,
                    &state.rerank_config,
                    query,
                    &search_queries,
                    top_k,
                    top_k,
                    &state.metrics,
                    &scope,
                )
                .await
            }
        }
    };
    let results = match retrieval {
        Ok(results) => results,
        Err(e) => {
            tracing::error!(error = %e, channel = %channel_id, "public search retrieval failed");
            return Err(ApiError::internal(INTERNAL_ERROR_MESSAGE));
        }
    };

    let items: Vec<SearchResultItem> = results.into_iter().map(SearchResultItem::from).collect();
    tracing::info!(
        channel = %channel_id,
        matched = items.len(),
        elapsed_ms = start.elapsed().as_millis() as u64,
        "public search finished"
    );
    Ok(Json(SearchResponse { results: items }))
}
