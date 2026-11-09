use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::StreamExt;
use opentelemetry::KeyValue;
use rig::streaming::{Item as StreamItem, StreamEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_stream::wrappers::ReceiverStream;
use tracing::Instrument;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::low_recall::LowRecallSource;
use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use rwiki_core::domain::chat::{evict_expired_sessions, ChatMessage};
use rwiki_core::domain::search::{
    max_relevance_score, rewrite_query, search_and_rerank, strip_markdown_fences,
};
use rwiki_core::infrastructure::vector_store::SearchResult;

/// Post-answer suggestion LLM call timeout (ms)
const POST_ANSWER_TIMEOUT_MS: u64 = 8000;
/// Post-answer suggestion LLM call max output tokens
const POST_ANSWER_MAX_TOKENS: usize = 200;
/// Maximum number of post-answer suggested questions to emit
const POST_ANSWER_MAX_QUESTIONS: usize = 3;

/// Public chat request body: restricts retrieval to the given channelId(s),
/// hitting only published documents of the specified channels.
///
/// `channel_id` is a list of channel identifiers supporting cross-channel
/// union retrieval; the handler validates each channel is configured
/// (missing or any unconfigured channel returns 400).
#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatRequest {
    pub message: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    /// Channel identifiers; required (re-validated in the handler to return 400).
    /// Supports single- or multi-channel union retrieval.
    #[serde(rename = "channelId", default)]
    pub channel_id: Option<Vec<String>>,
    /// Client capability declaration: when true, the server emits a
    /// `contentEnd` SSE event once the answer body completes. Absent/false
    /// keeps the legacy event set — old clients structurally never receive
    /// the new event.
    #[serde(rename = "supportsContentEndEvent", default)]
    pub supports_content_end_event: bool,
}

/// Request body for the authenticated `/api/chat/scoped` endpoint: allows
/// targeting a document set via documentIds, bypassing the published-only
/// restriction (builds RetrievalScope::Collection).
#[derive(Debug, Deserialize, ToSchema)]
pub struct ScopedChatRequest {
    pub message: String,
    #[serde(rename = "sessionId", default)]
    pub session_id: Option<String>,
    /// Retrieve from a specific document set (bypasses the published-only
    /// restriction); authenticated endpoint only
    #[serde(rename = "documentIds", default)]
    pub document_ids: Option<Vec<String>>,
    /// Client capability declaration: same semantics as `ChatRequest`.
    #[serde(rename = "supportsContentEndEvent", default)]
    pub supports_content_end_event: bool,
}

/// Deserialize a query field that may appear as a single value (`?channelId=a`)
/// or repeated (`?channelId=a&channelId=b`) into a flat `Vec<String>`.
///
/// axum's `Query` extractor uses serde_urlencoded, which hands a `Vec<String>`-typed
/// field a sequence only when the key repeats. A lone `?channelId=a` would fail to
/// deserialize as `Vec<String>`. This helper accepts both shapes so the public
/// suggestions endpoint stays backward-compatible with single-value callers (e.g.
/// the main site `/c/$channelId` route) while supporting multi-channel queries.
fn deserialize_channel_id_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StrOrVec {
        Single(String),
        Multi(Vec<String>),
    }

    Ok(match StrOrVec::deserialize(deserializer)? {
        StrOrVec::Single(s) => vec![s],
        StrOrVec::Multi(v) => v,
    })
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SuggestionsQuery {
    pub locale: Option<String>,
    /// Channel identifiers; required (accepts a single query value `?channelId=a`
    /// or repeated values `?channelId=a&channelId=b`)
    #[serde(rename = "channelId", deserialize_with = "deserialize_channel_id_vec")]
    pub channel_id: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SuggestionsResponse {
    pub questions: Vec<String>,
}

// SSE event types -- Serialize only, no ToSchema (utoipa cannot represent SSE schemas)

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionEvent {
    session_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChunkEvent {
    content: String,
}

#[derive(Debug, Serialize)]
struct DoneEvent {}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEvent {
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuggestionsEvent {
    suggestions: Vec<String>,
}

/// 回答主体内容完成信号（仅当请求声明 `supportsContentEndEvent=true` 时发送）。
/// payload 必须带判别键 `contentEnd`：真实客户端按 payload 形状（而非事件名）
/// 分发事件，空对象会与 `done` 的 `{}` 不可区分。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentEndEvent {
    content_end: bool,
}

/// Maximum number of suggested questions to return.
const MAX_SUGGESTIONS: usize = 10;

/// Validate locale format without regex.
/// Valid: 2-4 letters, optionally followed by `-` and 2-8 letters. Max 10 chars total.
/// Invalid formats (empty, digits, special chars, too long) are rejected.
fn is_valid_locale(locale: &str) -> bool {
    if locale.is_empty() || locale.len() > 10 {
        return false;
    }
    let bytes = locale.as_bytes();
    // First segment: 2-4 letters
    let mut i = 0;
    let first_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let first_len = i - first_start;
    if !(2..=4).contains(&first_len) {
        return false;
    }
    if i == bytes.len() {
        return true; // just the language part, e.g. "en"
    }
    // Must be a hyphen
    if bytes[i] != b'-' {
        return false;
    }
    i += 1;
    // Second segment: 2-8 letters
    let second_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    let second_len = i - second_start;
    if !(2..=8).contains(&second_len) {
        return false;
    }
    // No trailing characters allowed
    i == bytes.len()
}

/// Match locale against suggested_questions config.
///
/// Matching logic: exact match -> longest prefix match (key is a prefix of locale, pick longest)
/// -> "default" key -> empty vec.
/// Results are truncated to MAX_SUGGESTIONS.
pub(crate) fn match_locale(
    config: &Option<HashMap<String, Vec<String>>>,
    locale: Option<&str>,
) -> Vec<String> {
    let map = match config {
        Some(m) if !m.is_empty() => m,
        _ => return Vec::new(),
    };

    // Validate locale if provided
    let locale = locale
        .filter(|l| is_valid_locale(l))
        .map(|l| l.to_lowercase());

    let questions = if let Some(ref loc) = locale {
        // 1. Exact match (case-insensitive)
        if let Some(qs) = map.get(loc) {
            Some(qs)
        } else {
            // 2. Longest prefix match: find keys that are a prefix of the locale, pick longest
            let mut best_key: Option<&String> = None;
            let mut best_len = 0;
            for key in map.keys() {
                let key_lower = key.to_lowercase();
                if loc.starts_with(&key_lower)
                    && key_lower.len() > best_len
                    && is_valid_locale(&key_lower)
                {
                    best_key = Some(key);
                    best_len = key_lower.len();
                }
            }
            best_key
                .and_then(|k| map.get(k))
                .or_else(|| map.get("default"))
        }
    } else {
        // No locale provided, fall back to "default"
        map.get("default")
    };

    match questions {
        Some(qs) => qs.iter().take(MAX_SUGGESTIONS).cloned().collect(),
        None => Vec::new(),
    }
}

/// Get channel-level suggested questions for a given locale across one or more channels.
///
/// Returns the union of each configured channel's suggested questions for the locale,
/// de-duplicated (preserving first-seen order) and truncated to MAX_SUGGESTIONS.
/// Returns an empty list when none of the channels have configured questions.
/// Does NOT fall back to global/widget suggestions.
#[utoipa::path(
    get,
    path = "/api/chat/suggestions",
    tag = "chat",
    params(SuggestionsQuery),
    responses(
        (status = 200, description = "Suggested questions", body = SuggestionsResponse),
        (status = 400, description = "Invalid channelId", body = ErrorResponse)
    )
)]
pub async fn suggestions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SuggestionsQuery>,
) -> Result<Json<SuggestionsResponse>, ApiError> {
    let channel_ids = state
        .channels_config
        .require_all_configured(&params.channel_id)
        .map_err(|e| match e {
            rwiki_core::config::ChannelValidationError::Empty => {
                ApiError::bad_request("channelId 不能为空")
            }
            rwiki_core::config::ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?;

    // 跨频道合并：逐个频道解析 locale 匹配的问题，按频道顺序累加、去重（保首见顺序），
    // 最后截断到 MAX_SUGGESTIONS。
    let mut seen = std::collections::HashSet::new();
    let mut questions: Vec<String> = Vec::new();
    for channel_id in &channel_ids {
        let channel_questions: Option<HashMap<String, Vec<String>>> = state
            .channels_config
            .get(channel_id)
            .and_then(|s| s.suggested_questions.clone());
        for q in match_locale(&channel_questions, params.locale.as_deref()) {
            if seen.insert(q.clone()) {
                questions.push(q);
            }
        }
    }
    questions.truncate(MAX_SUGGESTIONS);
    Ok(Json(SuggestionsResponse { questions }))
}

/// 构建系统提示词，拼接角色/规则模板、可选摘要与 RAG 上下文。
/// 摘要（如有）插入在 system_prompt 与 RAG 上下文之间。
pub(crate) fn build_preamble(
    system_prompt: &str,
    summary: Option<&str>,
    rag_context: &str,
) -> String {
    let mut parts = vec![system_prompt.to_string()];
    if let Some(s) = summary {
        parts.push(format!("\nConversation Summary:\n{s}"));
    }
    parts.push(format!("\nContext:\n{rag_context}"));
    parts.join("\n")
}

/// 构建摘要压缩提示词。将已有的摘要（如有）与待压缩的旧消息拼接，
/// 引导 LLM 生成或更新摘要。
pub(crate) fn build_compact_prompt(
    existing_summary: Option<&str>,
    old_messages: &[ChatMessage],
) -> String {
    let mut parts = Vec::new();
    if let Some(summary) = existing_summary {
        parts.push(format!("当前摘要:\n{summary}"));
    }
    let messages_text = old_messages
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n");
    parts.push(format!("待压缩的对话历史:\n{messages_text}"));
    parts.join("\n\n")
}

/// Build the prompt for post-answer suggested questions generation.
/// Constraints the model to strict JSON `{"questions": [...]}`, ≤3, grounded in
/// the round's user message, answer, and retrieved context.
pub(crate) fn build_post_answer_prompt(
    user_message: &str,
    assistant_text: &str,
    context_xml: &str,
) -> String {
    format!(
        "用户问题: {user_message}\n\n\
         回答: {assistant_text}\n\n\
         检索到的知识库上下文:\n{context_xml}\n\n\
         基于以上用户问题、回答与知识库上下文，生成最多 {POST_ANSWER_MAX_QUESTIONS} 条用户可能想继续追问的问题。\n\
         要求：紧扣上述上下文；不要重复用户本轮已问的问题；不要推荐回答已完整覆盖的问题。\n\
         输出严格的 JSON 格式：{{\"questions\": [\"问题1\", \"问题2\", \"问题3\"]}}\n\
         最多 {POST_ANSWER_MAX_QUESTIONS} 条，可为更少。只输出 JSON，不要输出其他内容。"
    )
}

/// Parse the post-answer suggestion LLM response into ≤3 questions.
/// Steps: strip ```json fences -> parse `{"questions":[...]}` -> drop empties ->
/// dedupe (preserve order) -> truncate to `POST_ANSWER_MAX_QUESTIONS`.
/// KEY DIFFERENCE from `parse_rewrite_response`: on any parse failure or missing
/// `questions` key, returns an EMPTY Vec (NO raw-string fallback).
pub(crate) fn parse_suggested_questions_response(raw: &str) -> Vec<String> {
    let json_str = strip_markdown_fences(raw);

    let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) else {
        return Vec::new();
    };
    let Some(arr) = val.get("questions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<String> = Vec::new();
    for v in arr {
        if let Some(s) = v.as_str() {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                continue;
            }
            if seen.insert(trimmed) {
                out.push(trimmed.to_string());
            }
        }
    }
    out.truncate(POST_ANSWER_MAX_QUESTIONS);
    out
}

fn preview_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Preamble for the post-answer suggestion generator.
const PA_PREAMBLE: &str =
    "You are an assistant that generates follow-up questions. Output only strict JSON.";

/// Non-streaming generation of post-answer suggested questions.
/// Timeout / LLM error / parse failure -> empty Vec (silent degrade).
/// Reuses `strip_markdown_fences` via `parse_suggested_questions_response`.
/// No new metrics field; failure logged via `tracing::warn!` only.
pub(crate) async fn generate_post_answer_suggestions(
    llm_client: &rig::providers::openai::OpenAI,
    llm_model: &str,
    user_message: &str,
    assistant_text: &str,
    context_xml: &str,
) -> Vec<String> {
    let prompt = build_post_answer_prompt(user_message, assistant_text, context_xml);

    let agent = rig::AgentBuilder::new(llm_client.chat(llm_model))
        .preamble(PA_PREAMBLE)
        .max_tokens(POST_ANSWER_MAX_TOKENS as u64)
        .build();

    match tokio::time::timeout(
        Duration::from_millis(POST_ANSWER_TIMEOUT_MS),
        agent.prompt(&prompt),
    )
    .await
    {
        Ok(Ok(raw)) => parse_suggested_questions_response(&raw.output),
        Ok(Err(e)) => {
            tracing::warn!("post-answer suggestions failed: {e}");
            Vec::new()
        }
        Err(_) => {
            tracing::warn!("post-answer suggestions timed out after {POST_ANSWER_TIMEOUT_MS}ms");
            Vec::new()
        }
    }
}

/// Format search results into XML context string for LLM consumption.
pub(crate) fn format_context_xml(results: &[SearchResult]) -> String {
    format!(
        "<documents>\n{}\n</documents>",
        results
            .iter()
            .enumerate()
            .map(|(i, r)| format_context_block(i + 1, r))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

/// Build a session storage key that scopes memory sessions by channel(s).
///
/// Channel-scoped public chat uses `channel:{sorted_channel_ids_joined}:{session_id}`,
/// where the channel ids are joined by `,`. Multi-channel requests key on the
/// **sorted, comma-joined** id list so the same set of channels (regardless of input
/// order) maps to one session bucket — preventing cross-channel-combination cross-talk.
/// Scoped/internal chat uses `scoped:{session_id}`.
pub(crate) fn session_key(channel_id: Option<&[String]>, session_id: &str) -> String {
    match channel_id {
        Some(ids) if !ids.is_empty() => {
            format!("channel:{}:{session_id}", ids.join(","))
        }
        _ => format!("scoped:{session_id}"),
    }
}

/// 持久化一轮对话到会话记忆：user 消息总是写入；assistant 消息仅在非空时
/// 写入，`interrupted=true` 时标记为客户端断开导致的截断（正常完成为 false）。
/// 不触发压缩检查——单轮最多新增两条消息，超阈会话由下一个正常完成轮次的
/// 既有 should_compact 检查补上。按值接收消息文本：三个调用点在持久化后
/// 都不再使用原字符串，避免整段克隆。
async fn persist_round(
    chat_sessions: &tokio::sync::Mutex<HashMap<String, rwiki_core::domain::chat::ChatSession>>,
    storage_key: &str,
    user_message: String,
    assistant_text: String,
    interrupted: bool,
) {
    let mut sessions = chat_sessions.lock().await;
    let session = sessions
        .entry(storage_key.to_string())
        .or_insert_with_key(|id| rwiki_core::domain::chat::ChatSession::new(id.clone()));
    session.add_message("user", user_message);
    if !assistant_text.is_empty() {
        if interrupted {
            session.add_interrupted_assistant_message(assistant_text);
        } else {
            session.add_message("assistant", assistant_text);
        }
    }
}

/// 共享的 SSE 聊天主体：解析完请求、确定作用域之后的全部逻辑。
/// public `/api/chat` 与认证的 `/api/chat/scoped` 均通过此函数复用，
/// 仅检索作用域不同（前者恒为 Channel(s)，后者可由 documentIds 构建 Collection）。
///
/// `channel_id` 为已排序（字典序）、去重的频道列表；用于派生 session_key、
/// 系统 prompt 解析与低召回记录。单频道请求以单元素切片传入。
async fn chat_inner(
    state: Arc<AppState>,
    message: String,
    session_id: Option<String>,
    channel_id: Option<&[String]>,
    scope: rwiki_core::infrastructure::vector_store::RetrievalScope,
    supports_content_end_event: bool,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // Validate message is not empty
    if message.trim().is_empty() {
        state
            .metrics
            .chat_error_count
            .add(1, &[KeyValue::new("error_type", "empty_message")]);
        return Err(ApiError::bad_request("消息不能为空"));
    }

    // Check knowledge base is not empty（预检查自身失败向上抛 500，不当
    // 作「空库」误报 503）
    if state
        .vector_store
        .is_empty()
        .await
        .map_err(ApiError::from)?
    {
        state
            .metrics
            .chat_error_count
            .add(1, &[KeyValue::new("error_type", "no_index_data")]);
        return Err(ApiError::service_unavailable(
            "当前知识库中没有索引数据，请先上传文档",
        ));
    }

    // Determine session ID
    let is_new_session = session_id.is_none();
    let session_id = session_id.unwrap_or_else(|| Uuid::now_v7().to_string());
    let storage_key = session_key(channel_id, &session_id);
    let chat_span = tracing::info_span!(
        "chat_request",
        session_id = %session_id,
        channel_id = ?channel_id,
        storage_key = %storage_key,
        user_message_preview = %preview_chars(&message, 50),
        user_message_len = message.chars().count(),
        is_new_session,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
    );

    async move {
        let chat_start = Instant::now();
        state
            .metrics
            .chat_request_count
            .add(1, &[KeyValue::new("is_new_session", is_new_session)]);

        // Get session state: summary + history (with eviction + touch), then release lock
        let (summary, history) = {
            let mut sessions = state.chat_sessions.lock().await;
            evict_expired_sessions(&mut sessions);
            if let Some(session) = sessions.get_mut(&storage_key) {
                session.touch();
                (session.summary.clone(), session.messages.clone())
            } else {
                (None, Vec::new())
            }
        };

        // Query rewriting: always rewrite (unconditional)
        // content_language 频道级优先（多频道取排序后首个频道的值）、回退 chat 全局配置。
        let content_language = state.channels_config.resolved_content_language_multi(
            channel_id,
            state.chat_config.content_language.as_deref(),
        );
        let search_queries = rewrite_query(
            &state.llm_client,
            &state.llm_model,
            &message,
            &history,
            content_language,
            &state.metrics,
        )
        .await;

        // Hybrid search with keyword + vector fusion + optional rerank
        tracing::debug!("search queries: {:?}", search_queries);
        let search_results = search_and_rerank(
            &state.vector_store,
            &state.reranker,
            &state.rerank_config,
            &message,
            &search_queries,
            state.retrieval_config.search_top_k_per_query.max(1),
            state.retrieval_config.max_context_chunks.max(1),
            &state.metrics,
            &scope,
        )
        .await?;
        tracing::debug!(
            "search returned {} chunks for queries={:?}",
            search_results.len(),
            search_queries
        );
        for r in &search_results {
            tracing::debug!(
                "  context chunk: chunk_id={}, page_id={}, sub_index={:?}, title={:?}, score={:.4}",
                r.chunk_id,
                r.page_id,
                r.sub_index,
                r.title,
                r.score,
            );
        }

        // 旁路：低相关召回记录（不阻塞回答；仅公开 Channel(s) 作用域；仅功能启用时）
        // 写入为 detached tokio::spawn，chat 不 join/不 await，与 SSE 流式回答并发；
        // 返回 Err 仅 warn、不传播；任务 panic 由 tokio 隔离——均不影响 chat 回答。
        //
        // 多频道场景下，low_recall_records.channel_id（单 TEXT 列）记录排序后的**首个**
        // 频道 id，使 list 查询行为保持单频道筛选语义不变。
        let low_recall_channel_id: Option<String> = match &scope {
            rwiki_core::infrastructure::vector_store::RetrievalScope::Channel(channel_id) => {
                Some(channel_id.clone())
            }
            rwiki_core::infrastructure::vector_store::RetrievalScope::Channels(channel_ids) => {
                channel_ids.first().cloned()
            }
            _ => None,
        };
        if let Some(channel_id_for_low_recall) = low_recall_channel_id {
            if let Some(lr_cfg) = state.low_recall_config.as_ref() {
                // threshold 语义是 top-1 检索相关度（config 文档）；最终排序按
                // page 粒度（列表首条未必最高分），且 TITLE_MATCH_BONUS 是排序
                // 加权而非相关度（虽已写入对外得分），故取剔除 bonus 后的最高
                // 相关度。None = 完全未命中（零命中必记的口径不变）。
                let mut rank_queries: Vec<&str> = vec![&message];
                rank_queries.extend(search_queries.iter().map(String::as_str));
                let top_score = max_relevance_score(&search_results, &rank_queries);
                let should_log = top_score.is_none_or(|s| s < lr_cfg.threshold); // 无结果必记
                if should_log {
                    let query = message.clone();
                    let session_id = session_id.clone();
                    let result_count = search_results.len() as i64;
                    // top-K 来源摘要：按相关度取前 5（与 top_score 同口径；
                    // page 粒度排序后列表序是 page 顺序，直接 take(5) 会漏掉
                    // 最高分来源）
                    let mut by_score: Vec<_> = search_results.iter().collect();
                    by_score.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    let sources: Vec<LowRecallSource> = by_score
                        .iter()
                        .take(5)
                        .map(|r| LowRecallSource {
                            document_id: r.document_id.clone(),
                            chunk_id: r.chunk_id.clone(),
                            title: r.title.clone(),
                            score: r.score,
                        })
                        .collect();
                    let sqlite = state.sqlite.clone();
                    tokio::spawn(async move {
                        let sources_json =
                            serde_json::to_string(&sources).unwrap_or_else(|_| "[]".to_string());
                        let res = sqlite
                            .call(move |conn| {
                                conn.execute(
                                    "INSERT INTO low_recall_records \
                                     (session_id, channel_id, query, top_score, result_count, sources) \
                                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                                    rusqlite::params![
                                        session_id,
                                        channel_id_for_low_recall,
                                        query,
                                        top_score,
                                        result_count,
                                        sources_json,
                                    ],
                                )?;
                                Ok::<(), rusqlite::Error>(())
                            })
                            .await;
                        if let Err(e) = res {
                            tracing::warn!(
                                error = %e,
                                "low-recall record write failed (bypass, chat unaffected)"
                            );
                        }
                    });
                }
            }
        }

        let context_text = format_context_xml(&search_results);
        let context_chunks = search_results.len();
        let context_chars = context_text.chars().count();

        let system_prompt = state
            .channels_config
            .resolved_system_prompt_multi(channel_id, &state.chat_config.system_prompt);
        let preamble = build_preamble(system_prompt, summary.as_deref(), &context_text);

        // Build per-request agent with context in preamble
        let agent = rig::AgentBuilder::new(state.llm_client.chat(&state.llm_model))
            .preamble(&preamble)
            .build();

        // Build chat history from sliding window only
        let sliding_window_size = state.chat_config.sliding_window_size;
        let sliding_window = {
            let sessions = state.chat_sessions.lock().await;
            if let Some(session) = sessions.get(&storage_key) {
                session.get_sliding_window(sliding_window_size).to_vec()
            } else {
                Vec::new()
            }
        };
        let chat_history: Vec<rig::completion::Message> = sliding_window
            .iter()
            .map(|msg| {
                if msg.role == "user" {
                    rig::completion::Message::from(msg.content.clone())
                } else {
                    rig::completion::Message::from(rig::message::AssistantContent::text(
                        &msg.content,
                    ))
                }
            })
            .collect();

        let user_message = message.clone();

        // Extract config values before spawn (state moves into the closure)
        let compact_threshold = state.chat_config.compact_threshold;
        let token_budget = state.chat_config.token_budget;
        let llm_model_for_span = state.llm_model.clone();
        let enable_post_answer_suggestions = state.chat_config.enable_post_answer_suggestions;

        // Spawn a task to stream LLM response
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

        let chat_span = tracing::Span::current();
        tokio::spawn(
            async move {
                let chat_request_span = tracing::Span::current();
                let llm_span = tracing::info_span!(
                    "llm_generate",
                    model = %llm_model_for_span,
                    context_chunks,
                    context_chars,
                    output_chars = tracing::field::Empty,
                    first_token_latency_ms = tracing::field::Empty,
                    error = tracing::field::Empty,
                    error.message = tracing::field::Empty,
                );

                async move {
                    // Send session event
                    let session_event = SessionEvent {
                        session_id: session_id.clone(),
                    };
                    let event = Event::default()
                        .event("session")
                        .data(serde_json::to_string(&session_event).unwrap_or_default());
                    if tx.send(Ok(event)).await.is_err() {
                        // Disconnected before any content: persist user message only
                        persist_round(
                            &state.chat_sessions,
                            &storage_key,
                            user_message,
                            String::new(),
                            true,
                        )
                        .await;
                        return;
                    }

                    // Stream LLM response using agent runner stream (with original user message)
                    let llm_started_at = Instant::now();
                    let mut first_text_chunk_seen = false;
                    let mut stream = agent
                        .prompt(user_message.clone())
                        .history(chat_history)
                        .stream();
                    let mut assistant_text = String::new();

                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                                StreamItem::Event(StreamEvent::Text { text, .. }),
                            )) => {
                                if !first_text_chunk_seen {
                                    first_text_chunk_seen = true;
                                    let first_token_ms =
                                        llm_started_at.elapsed().as_secs_f64() * 1000.0;
                                    tracing::Span::current()
                                        .record("first_token_latency_ms", first_token_ms as u64);
                                    state
                                        .metrics
                                        .llm_first_token_duration
                                        .record(first_token_ms, &[]);
                                }
                                assistant_text.push_str(&text);
                                let chunk_event = ChunkEvent { content: text };
                                let event = Event::default()
                                    .event("chunk")
                                    .data(serde_json::to_string(&chunk_event).unwrap_or_default());
                                if tx.send(Ok(event)).await.is_err() {
                                    tracing::Span::current()
                                        .record("output_chars", assistant_text.chars().count());
                                    // Client disconnected mid-stream: persist the round with
                                    // whatever partial answer was generated so far
                                    persist_round(
                                        &state.chat_sessions,
                                        &storage_key,
                                        user_message,
                                        assistant_text,
                                        true,
                                    )
                                    .await;
                                    return;
                                }
                            }
                            Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)) => {
                                // Capability-gated content-complete signal: emitted after the
                                // last chunk, before suggestion generation. Send errors are
                                // ignored — the answer is complete and normal persistence
                                // proceeds regardless of whether the client is still there.
                                if supports_content_end_event {
                                    let event = Event::default().event("contentEnd").data(
                                        serde_json::to_string(&ContentEndEvent {
                                            content_end: true,
                                        })
                                        .unwrap_or_default(),
                                    );
                                    let _ = tx.send(Ok(event)).await;
                                }
                                // Post-answer suggestions (only when switch is on and the
                                // client is still connected; silent degrade to empty)
                                if enable_post_answer_suggestions && !tx.is_closed() {
                                    let suggestions = generate_post_answer_suggestions(
                                        &state.llm_client,
                                        &state.llm_model,
                                        &user_message,
                                        &assistant_text,
                                        &context_text,
                                    )
                                    .await;
                                    if !suggestions.is_empty() {
                                        let event = Event::default().event("suggestions").data(
                                            serde_json::to_string(&SuggestionsEvent {
                                                suggestions,
                                            })
                                            .unwrap_or_default(),
                                        );
                                        let _ = tx.send(Ok(event)).await;
                                    }
                                }
                                // Stream complete, send done event
                                let done_event = DoneEvent {};
                                let event = Event::default()
                                    .event("done")
                                    .data(serde_json::to_string(&done_event).unwrap_or_default());
                                let _ = tx.send(Ok(event)).await;
                                break;
                            }
                            Ok(_) => {
                                // Ignore other stream items (tool calls, reasoning, etc.)
                            }
                            Err(e) => {
                                tracing::error!("Stream error: {e}");
                                tracing::Span::current().record("error", true);
                                tracing::Span::current().record("error.message", e.to_string());
                                chat_request_span.record("error", true);
                                chat_request_span.record("error.message", e.to_string());
                                state.metrics.llm_error_count.add(1, &[]);
                                state
                                    .metrics
                                    .chat_error_count
                                    .add(1, &[KeyValue::new("error_type", "llm_stream")]);
                                let error_event = ErrorEvent {
                                    message: "Failed to generate response. Please try again later."
                                        .to_string(),
                                };
                                let event = Event::default()
                                    .event("error")
                                    .data(serde_json::to_string(&error_event).unwrap_or_default());
                                let _ = tx.send(Ok(event)).await;
                                tracing::Span::current()
                                    .record("output_chars", assistant_text.chars().count());
                                return;
                            }
                        }
                    }
                    tracing::Span::current().record("output_chars", assistant_text.chars().count());
                    state
                        .metrics
                        .llm_output_chars
                        .record(assistant_text.chars().count() as f64, &[]);
                    state
                        .metrics
                        .llm_context_chunks
                        .record(context_chunks as f64, &[]);

                    // Persist user message and assistant response to session
                    persist_round(
                        &state.chat_sessions,
                        &storage_key,
                        user_message,
                        assistant_text,
                        false,
                    )
                    .await;

                    // Compact check: if session exceeds thresholds, compress old messages
                    let sessions = state.chat_sessions.clone();
                    let llm_client = state.llm_client.clone();
                    let llm_model = state.llm_model.clone();
                    {
                        let mut sessions_lock = sessions.lock().await;
                        if let Some(session) = sessions_lock.get_mut(&storage_key) {
                            if session.should_compact(
                                compact_threshold,
                                token_budget,
                                sliding_window_size,
                            ) {
                                let old_count =
                                    session.messages.len().saturating_sub(sliding_window_size);
                                let old_messages = session.messages[..old_count].to_vec();
                                let existing_summary = session.summary.clone();
                                drop(sessions_lock); // release lock before LLM call

                                let prompt = build_compact_prompt(
                                    existing_summary.as_deref(),
                                    &old_messages,
                                );
                                let compact_agent = rig::AgentBuilder::new(
                                    llm_client.chat(&llm_model),
                                )
                                .preamble(
                                    "请将以下对话历史压缩为一段简洁的摘要，保留关键信息、事实和上下文。",
                                )
                                .max_tokens(300)
                                .build();
                                match compact_agent.prompt(&prompt).await {
                                    Ok(response) => {
                                        let new_summary = response.output;
                                        if let Some(session) =
                                            sessions.lock().await.get_mut(&session_id)
                                        {
                                            session
                                                .compact_history(new_summary, sliding_window_size);
                                        } else {
                                            tracing::warn!(
                                    "session {session_id} evicted during compact, skipping"
                                );
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            "compact failed: {e}, keeping original messages"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    state
                        .metrics
                        .llm_duration
                        .record(llm_started_at.elapsed().as_secs_f64() * 1000.0, &[]);
                    state
                        .metrics
                        .chat_duration
                        .record(chat_start.elapsed().as_secs_f64() * 1000.0, &[]);
                }
                .instrument(llm_span)
                .await;
            }
            .instrument(chat_span),
        );

        let stream = ReceiverStream::new(rx);
        Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
    }
    .instrument(chat_span)
    .await
}

/// Public SSE chat endpoint (no authentication required).
///
/// Retrieves published content for the configured `channelId` only (RetrievalScope::Channel).
#[utoipa::path(
    post,
    path = "/api/chat",
    tag = "chat",
    request_body = ChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat response events", content_type = "text/event-stream"),
        (status = 400, description = "Invalid request or channel", body = ErrorResponse),
        (status = 503, description = "Knowledge base is empty or channel has no published documents", body = ErrorResponse)
    )
)]
pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // 批量校验：去重 + 字典序排序，保证同一组频道（不论输入顺序）映射到稳定的作用域与 session。
    let channel_ids = state
        .channels_config
        .require_all_configured(req.channel_id.as_deref().unwrap_or(&[]))
        .map_err(|e| match e {
            rwiki_core::config::ChannelValidationError::Empty => {
                ApiError::bad_request("channelId 不能为空")
            }
            rwiki_core::config::ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?;

    if !state
        .vector_store
        .has_published_documents_for_channels(&channel_ids)
        .await
    {
        return Err(ApiError::service_unavailable("当前频道没有可用文档"));
    }

    chat_inner(
        state,
        req.message,
        req.session_id,
        Some(&channel_ids),
        rwiki_core::infrastructure::vector_store::RetrievalScope::Channels(channel_ids.clone()),
        req.supports_content_end_event,
    )
    .await
}

/// Authenticated SSE chat endpoint `/api/chat/scoped` (requires API key).
///
/// Allows specifying a document collection via documentIds, building a
/// RetrievalScope::Collection that bypasses the published-only restriction.
#[utoipa::path(
    post,
    path = "/api/chat/scoped",
    tag = "chat",
    security(("bearer_auth" = [])),
    request_body = ScopedChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat response events", content_type = "text/event-stream"),
        (status = 400, description = "Message cannot be empty", body = ErrorResponse),
        (status = 401, description = "Missing or invalid API key", body = ErrorResponse),
        (status = 503, description = "Knowledge base is empty", body = ErrorResponse)
    )
)]
pub async fn chat_scoped(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ScopedChatRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let scope = rwiki_core::infrastructure::vector_store::RetrievalScope::from_document_ids(
        req.document_ids.as_ref(),
    );
    chat_inner(
        state,
        req.message,
        req.session_id,
        None,
        scope,
        req.supports_content_end_event,
    )
    .await
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_context_block(index: usize, result: &SearchResult) -> String {
    let mut tags = Vec::new();

    tags.push(format!(r#"<document index="{}">"#, index));
    tags.push(format!("<title>{}</title>", escape_xml(&result.title)));

    if let Some(section) = &result.section {
        if !section.is_empty() {
            tags.push(format!("<section>{}</section>", escape_xml(section)));
        }
    }

    if let Some(link) = &result.link {
        if !link.is_empty() {
            tags.push(format!("<link>{}</link>", escape_xml(link)));
        }
    }

    if let Some(locale) = &result.locale {
        if !locale.is_empty() {
            tags.push(format!("<locale>{}</locale>", escape_xml(locale)));
        }
    }

    tags.push(format!(
        "<content>\n{}\n</content>",
        escape_xml(&result.content)
    ));
    tags.push("</document>".to_string());

    tags.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a SearchResult for testing.
    fn make_result(
        title: &str,
        section: Option<&str>,
        link: Option<&str>,
        locale: Option<&str>,
        content: &str,
    ) -> SearchResult {
        SearchResult {
            chunk_id: "test-chunk".to_string(),
            content: content.to_string(),
            score: 0.9,
            document_id: "test-doc".to_string(),
            page_id: "test-page-id".to_string(),
            sub_index: None,
            chunk_count: None,
            title: title.to_string(),
            locale: locale.map(|s| s.to_string()),
            link: link.map(|s| s.to_string()),
            tags: vec![],
            section: section.map(|s| s.to_string()),
        }
    }

    // --- format_context_block tests ---

    #[test]
    fn format_context_block_full_metadata() {
        let result = make_result(
            "Getting Started",
            Some("Installation"),
            Some("https://example.com/docs"),
            Some("zh-CN"),
            "Install via cargo.",
        );
        let output = format_context_block(1, &result);
        assert!(
            output.contains(r#"<document index="1">"#),
            "should contain document tag with index 1"
        );
        assert!(
            output.contains("<title>Getting Started</title>"),
            "should contain title tag"
        );
        assert!(
            output.contains("<section>Installation</section>"),
            "should contain section tag"
        );
        assert!(
            output.contains("<link>https://example.com/docs</link>"),
            "should contain link tag"
        );
        assert!(
            output.contains("<locale>zh-CN</locale>"),
            "should contain locale tag"
        );
        assert!(
            output.contains("<content>\nInstall via cargo.\n</content>"),
            "should contain content with blank lines"
        );
        assert!(output.contains("</document>"), "should close document tag");
    }

    #[test]
    fn format_context_block_no_link_no_locale() {
        let result = make_result(
            "Getting Started",
            Some("Installation"),
            None,
            None,
            "Install via cargo.",
        );
        let output = format_context_block(1, &result);
        assert!(
            output.contains("<title>Getting Started</title>"),
            "should contain title tag"
        );
        assert!(
            output.contains("<section>Installation</section>"),
            "should contain section tag"
        );
        assert!(
            !output.contains("<link>"),
            "should NOT contain link tag when link is None"
        );
        assert!(
            !output.contains("<locale>"),
            "should NOT contain locale tag when locale is None"
        );
        assert!(output.contains("Install via cargo."));
    }

    #[test]
    fn format_context_block_no_section_with_link() {
        let result = make_result(
            "Getting Started",
            None,
            Some("https://example.com/docs"),
            None,
            "Some content.",
        );
        let output = format_context_block(1, &result);
        assert!(
            output.contains("<title>Getting Started</title>"),
            "should contain title tag"
        );
        assert!(
            !output.contains("<section>"),
            "should NOT contain section tag when section is None"
        );
        assert!(
            output.contains("<link>https://example.com/docs</link>"),
            "should contain link tag"
        );
    }

    #[test]
    fn format_context_block_no_section_no_link() {
        let result = make_result("Getting Started", None, None, None, "Content here.");
        let output = format_context_block(1, &result);
        assert!(
            output.contains("<title>Getting Started</title>"),
            "should contain title tag"
        );
        assert!(!output.contains("<link>"), "should NOT contain link tag");
        assert!(
            !output.contains("<locale>"),
            "should NOT contain locale tag"
        );
        assert!(
            !output.contains("<section>"),
            "should NOT contain section tag"
        );
        assert!(output.contains("Content here."));
    }

    #[test]
    fn format_context_block_empty_link_string_omits_link_line() {
        let result = make_result(
            "Getting Started",
            Some("Installation"),
            Some(""),
            None,
            "Content.",
        );
        let output = format_context_block(1, &result);
        assert!(
            !output.contains("<link>"),
            "empty string link should NOT produce link tag"
        );
    }

    #[test]
    fn format_context_block_empty_locale_string_omits_locale_line() {
        let result = make_result(
            "Getting Started",
            Some("Installation"),
            None,
            Some(""),
            "Content.",
        );
        let output = format_context_block(1, &result);
        assert!(
            !output.contains("<locale>"),
            "empty string locale should NOT produce locale tag"
        );
    }

    #[test]
    fn format_context_block_mixed_results_with_and_without_metadata() {
        let results = [
            make_result(
                "Doc A",
                Some("Section A"),
                Some("https://a.com"),
                Some("en"),
                "Content A.",
            ),
            make_result("Doc B", None, None, None, "Content B."),
            make_result(
                "Doc C",
                Some("Section C"),
                Some("https://c.com"),
                None,
                "Content C.",
            ),
        ];

        let blocks: Vec<String> = results
            .iter()
            .enumerate()
            .map(|(i, r)| format_context_block(i + 1, r))
            .collect();

        // First result (index 1): has link and locale
        assert!(
            blocks[0].contains(r#"<document index="1">"#),
            "first result should have index 1"
        );
        assert!(
            blocks[0].contains("<link>https://a.com</link>"),
            "first result should have link tag"
        );
        assert!(
            blocks[0].contains("<locale>en</locale>"),
            "first result should have locale tag"
        );

        // Second result (index 2): no link, no locale, no section
        assert!(
            blocks[1].contains(r#"<document index="2">"#),
            "second result should have index 2"
        );
        assert!(
            !blocks[1].contains("<link>"),
            "second result should NOT have link tag"
        );
        assert!(
            !blocks[1].contains("<locale>"),
            "second result should NOT have locale tag"
        );
        assert!(
            !blocks[1].contains("<section>"),
            "second result should NOT have section tag"
        );

        // Third result (index 3): has link but no locale
        assert!(
            blocks[2].contains(r#"<document index="3">"#),
            "third result should have index 3"
        );
        assert!(
            blocks[2].contains("<link>https://c.com</link>"),
            "third result should have link tag"
        );
        assert!(
            !blocks[2].contains("<locale>"),
            "third result should NOT have locale tag"
        );
    }

    // --- build_preamble tests (expanded to 3 args) ---

    #[test]
    fn build_preamble_without_summary_omits_summary_section() {
        let preamble = build_preamble("System prompt.", None, "RAG context.");
        assert!(
            preamble.contains("System prompt."),
            "should contain system prompt"
        );
        assert!(
            preamble.contains("Context:\nRAG context."),
            "should contain RAG context"
        );
        assert!(
            !preamble.contains("Conversation Summary"),
            "should NOT contain summary section when summary is None"
        );
    }

    #[test]
    fn build_preamble_with_summary_includes_summary_section() {
        let preamble = build_preamble(
            "System prompt.",
            Some("Previously discussed Rust generics."),
            "RAG context.",
        );
        assert!(
            preamble.contains("Conversation Summary:\nPreviously discussed Rust generics."),
            "should include summary between system_prompt and RAG context"
        );
        assert!(
            preamble.contains("System prompt."),
            "should contain system prompt"
        );
        assert!(
            preamble.contains("Context:\nRAG context."),
            "should contain RAG context"
        );
    }

    #[test]
    fn build_preamble_with_empty_context_produces_valid_structure() {
        let preamble = build_preamble("Custom prompt.", None, "");
        assert!(
            preamble.starts_with("Custom prompt."),
            "preamble should start with system prompt even with empty context"
        );
    }

    #[test]
    fn build_preamble_with_empty_system_prompt_still_produces_structure() {
        let preamble = build_preamble("", None, "Some context.");
        assert!(
            preamble.contains("Context:\nSome context."),
            "preamble should preserve context even when system_prompt is empty"
        );
    }

    #[test]
    fn build_preamble_with_multiline_system_prompt_preserves_separator() {
        let system_prompt = "Line one.\nLine two.\nLine three.";
        let preamble = build_preamble(system_prompt, None, "Context.");
        assert!(
            preamble.contains("Context:\nContext."),
            "separator must exist even with multi-line system_prompt"
        );
    }

    // --- escape_xml tests ---

    #[test]
    fn escape_xml_escapes_all_special_chars() {
        let input = "&<>\"'";
        let output = escape_xml(input);
        assert_eq!(
            output, "&amp;&lt;&gt;&quot;&apos;",
            "should escape all 5 XML special characters"
        );
    }

    #[test]
    fn escape_xml_no_double_escape() {
        // After escaping < and >, the & in &lt; and &gt; should NOT be re-escaped
        let input = "<hello>";
        let output = escape_xml(input);
        assert_eq!(
            output, "&lt;hello&gt;",
            "should not double-escape: & first, then < and >"
        );
    }

    // --- format_context_block XML escape tests ---

    #[test]
    fn format_context_block_xml_escape_in_title() {
        let result = make_result(
            "<script>alert('xss')</script>",
            None,
            None,
            None,
            "Normal content.",
        );
        let output = format_context_block(1, &result);
        assert!(
            output.contains("<title>&lt;script&gt;alert(&apos;xss&apos;)&lt;/script&gt;</title>"),
            "title with <, >, ' should be escaped"
        );
        assert!(
            !output.contains("<title><script>"),
            "raw unescaped < should not appear in title"
        );
    }

    #[test]
    fn format_context_block_xml_escape_in_content() {
        let result = make_result("Title", None, None, None, "A & B < C > D");
        let output = format_context_block(1, &result);
        assert!(
            output.contains("A &amp; B &lt; C &gt; D"),
            "content with &, <, > should be escaped"
        );
    }

    #[test]
    fn format_context_block_numbering_starts_at_one() {
        let result = make_result("Title", None, None, None, "Content.");
        let output = format_context_block(1, &result);
        assert!(
            output.contains(r#"<document index="1">"#),
            "index should start at 1"
        );

        let output2 = format_context_block(3, &result);
        assert!(
            output2.contains(r#"<document index="3">"#),
            "index should reflect the passed value"
        );
    }

    #[test]
    fn format_context_block_wrapped_in_documents_tag() {
        let results = [
            make_result("Doc A", Some("Sec A"), None, None, "Content A."),
            make_result("Doc B", None, None, None, "Content B."),
        ];

        // Reproduce the call-site logic to test <documents> wrapper
        let context_text = format!(
            "<documents>\n{}\n</documents>",
            results
                .iter()
                .enumerate()
                .map(|(i, r)| format_context_block(i + 1, r))
                .collect::<Vec<_>>()
                .join("\n")
        );

        assert!(
            context_text.starts_with("<documents>\n"),
            "should start with <documents> tag"
        );
        assert!(
            context_text.ends_with("\n</documents>"),
            "should end with </documents> tag"
        );
        assert!(
            context_text.contains(r#"<document index="1">"#),
            "first document should have index 1"
        );
        assert!(
            context_text.contains(r#"<document index="2">"#),
            "second document should have index 2"
        );
        // Internal separator is \n, not \n\n
        assert!(
            !context_text.contains("</document>\n\n<document"),
            "documents should be separated by single newline, not double"
        );
    }

    // --- build_compact_prompt tests ---

    #[test]
    fn build_compact_prompt_with_existing_summary_includes_old_summary() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "What is Rust?".into(),
                interrupted: false,
            },
            ChatMessage {
                role: "assistant".into(),
                content: "A systems language.".into(),
                interrupted: false,
            },
        ];
        let prompt = build_compact_prompt(Some("Previous summary about Rust."), &messages);
        assert!(
            prompt.contains("当前摘要:\nPrevious summary about Rust."),
            "should include existing summary"
        );
        assert!(
            prompt.contains("user: What is Rust?"),
            "should include old messages"
        );
    }

    #[test]
    fn build_compact_prompt_without_existing_summary_omits_summary_section() {
        let messages = vec![ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
            interrupted: false,
        }];
        let prompt = build_compact_prompt(None, &messages);
        assert!(
            !prompt.contains("当前摘要"),
            "should NOT include summary section when no existing summary"
        );
        assert!(
            prompt.contains("待压缩的对话历史"),
            "should include old messages section"
        );
        assert!(
            prompt.contains("user: What is Rust?"),
            "should include old message content"
        );
    }

    // --- match_locale and is_valid_locale tests (BE-D01) ---

    // Covers: PRD locale validation — valid 2-letter locale accepted.
    #[test]
    fn is_valid_locale_two_letters() {
        assert!(is_valid_locale("en"), "2-letter locale should be valid");
        assert!(is_valid_locale("zh"), "2-letter locale should be valid");
    }

    // Covers: PRD locale validation — valid locale with region tag accepted.
    #[test]
    fn is_valid_locale_with_region() {
        assert!(
            is_valid_locale("zh-CN"),
            "locale with region should be valid"
        );
        assert!(
            is_valid_locale("en-US"),
            "locale with region should be valid"
        );
        assert!(
            is_valid_locale("pt-BR"),
            "locale with region should be valid"
        );
    }

    // Covers: PRD locale validation — 3-4 letter language codes accepted.
    #[test]
    fn is_valid_locale_three_four_letter_language() {
        assert!(is_valid_locale("eng"), "3-letter language should be valid");
        assert!(is_valid_locale("zhcn"), "4-letter language should be valid");
    }

    // Covers: PRD locale validation — single letter rejected.
    #[test]
    fn is_valid_locale_single_letter_rejected() {
        assert!(
            !is_valid_locale("e"),
            "single letter locale should be rejected"
        );
    }

    // Covers: PRD locale validation — empty string rejected.
    #[test]
    fn is_valid_locale_empty_rejected() {
        assert!(!is_valid_locale(""), "empty locale should be rejected");
    }

    // Covers: PRD locale validation — digits rejected.
    #[test]
    fn is_valid_locale_with_digits_rejected() {
        assert!(
            !is_valid_locale("en123"),
            "locale with digits should be rejected"
        );
        assert!(
            !is_valid_locale("12"),
            "all-digit locale should be rejected"
        );
    }

    // Covers: PRD locale validation — special characters rejected.
    #[test]
    fn is_valid_locale_with_special_chars_rejected() {
        assert!(
            !is_valid_locale("en_US"),
            "underscore is not a valid separator"
        );
        assert!(!is_valid_locale("en."), "dot should be rejected");
        assert!(!is_valid_locale("abc!"), "special char should be rejected");
    }

    // Covers: PRD locale validation — too long rejected.
    #[test]
    fn is_valid_locale_too_long_rejected() {
        assert!(
            !is_valid_locale("abcdefghijk"),
            "locale > 10 chars should be rejected"
        );
    }

    // Covers: PRD locale validation — trailing hyphen rejected.
    #[test]
    fn is_valid_locale_trailing_hyphen_rejected() {
        assert!(
            !is_valid_locale("en-"),
            "trailing hyphen should be rejected"
        );
    }

    // Covers: Design 4.2.2 — config None returns empty vec.
    #[test]
    fn match_locale_none_config_returns_empty() {
        let result = match_locale(&None, Some("en"));
        assert!(result.is_empty(), "None config should return empty vec");
    }

    // Covers: Design 4.2.2 — exact match returns matching questions.
    #[test]
    fn match_locale_exact_match() {
        let config = Some(HashMap::from([
            ("default".to_string(), vec!["Q default".to_string()]),
            ("zh-CN".to_string(), vec!["Q zh".to_string()]),
            ("en".to_string(), vec!["Q en".to_string()]),
        ]));
        let result = match_locale(&config, Some("zh-CN"));
        assert_eq!(result, vec!["Q zh"]);
    }

    // Covers: Design 4.2.2 — case-insensitive exact match.
    #[test]
    fn match_locale_case_insensitive() {
        let config = Some(HashMap::from([(
            "zh-CN".to_string(),
            vec!["Q zh".to_string()],
        )]));
        let result = match_locale(&config, Some("zh-cn"));
        assert_eq!(result, vec!["Q zh"]);
    }

    // Covers: Design 4.2.2 — longest prefix match when no exact match.
    #[test]
    fn match_locale_longest_prefix_match() {
        let config = Some(HashMap::from([
            ("zh".to_string(), vec!["Q zh short".to_string()]),
            ("zh-CN".to_string(), vec!["Q zh-CN".to_string()]),
        ]));
        // "zh-CN" exact match exists, so it takes priority
        let result = match_locale(&config, Some("zh-CN"));
        assert_eq!(result, vec!["Q zh-CN"]);

        // "zh-TW" has no exact match, "zh" is a prefix → prefix match
        let result = match_locale(&config, Some("zh-TW"));
        assert_eq!(result, vec!["Q zh short"]);
    }

    // Covers: Design 4.2.2 — default key fallback when no locale match.
    #[test]
    fn match_locale_default_fallback() {
        let config = Some(HashMap::from([
            ("default".to_string(), vec!["Q default".to_string()]),
            ("en".to_string(), vec!["Q en".to_string()]),
        ]));
        let result = match_locale(&config, Some("ja"));
        assert_eq!(result, vec!["Q default"]);
    }

    // Covers: Design 4.2.2 — no match and no default returns empty vec.
    #[test]
    fn match_locale_no_match_no_default_returns_empty() {
        let config = Some(HashMap::from([(
            "en".to_string(),
            vec!["Q en".to_string()],
        )]));
        let result = match_locale(&config, Some("ja"));
        assert!(
            result.is_empty(),
            "no match and no default should return empty"
        );
    }

    // Covers: Design 4.2.2 — None locale falls back to default.
    #[test]
    fn match_locale_none_locale_uses_default() {
        let config = Some(HashMap::from([
            ("default".to_string(), vec!["Q default".to_string()]),
            ("en".to_string(), vec!["Q en".to_string()]),
        ]));
        let result = match_locale(&config, None);
        assert_eq!(result, vec!["Q default"]);
    }

    // Covers: Design 4.2.2 — None locale without default returns empty.
    #[test]
    fn match_locale_none_locale_no_default_returns_empty() {
        let config = Some(HashMap::from([(
            "en".to_string(),
            vec!["Q en".to_string()],
        )]));
        let result = match_locale(&config, None);
        assert!(
            result.is_empty(),
            "None locale without default should return empty"
        );
    }

    // Covers: PRD — invalid locale falls back to default.
    #[test]
    fn match_locale_invalid_locale_uses_default() {
        let config = Some(HashMap::from([
            ("default".to_string(), vec!["Q default".to_string()]),
            ("en".to_string(), vec!["Q en".to_string()]),
        ]));
        let result = match_locale(&config, Some("abc123!"));
        assert_eq!(result, vec!["Q default"]);
    }

    // Covers: PRD — truncation to max 10 questions.
    #[test]
    fn match_locale_truncates_to_max_ten() {
        let questions: Vec<String> = (1..=15).map(|i| format!("Q{i}")).collect();
        let config = Some(HashMap::from([("default".to_string(), questions)]));
        let result = match_locale(&config, None);
        assert_eq!(result.len(), 10, "should truncate to 10 questions");
        assert_eq!(result[0], "Q1");
        assert_eq!(result[9], "Q10");
    }

    // Covers: PRD — empty config HashMap returns empty vec.
    #[test]
    fn match_locale_empty_map_returns_empty() {
        let config = Some(HashMap::new());
        let result = match_locale(&config, Some("en"));
        assert!(result.is_empty(), "empty HashMap should return empty vec");
    }

    // --- parse_suggested_questions_response tests (US-CORE-037) ---

    // Covers: valid plain JSON is parsed in order.
    #[test]
    fn parse_suggested_questions_valid_plain_json() {
        let raw = r#"{"questions":["A","B","C"]}"#;
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    }

    // Covers: ```json fence is stripped via strip_markdown_fences reuse.
    #[test]
    fn parse_suggested_questions_json_fenced() {
        let raw = "```json\n{\"questions\":[\"A\"]}\n```";
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string()]);
    }

    // Covers: bare ``` fence is stripped.
    #[test]
    fn parse_suggested_questions_bare_fenced() {
        let raw = "```\n{\"questions\":[\"A\"]}\n```";
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string()]);
    }

    // Covers: missing `questions` key -> empty Vec (NOT raw fallback; key diff from rewrite).
    #[test]
    fn parse_suggested_questions_missing_key_returns_empty() {
        let raw = r#"{"foo":"bar"}"#;
        let out = parse_suggested_questions_response(raw);
        assert!(
            out.is_empty(),
            "missing questions key must return empty Vec"
        );
    }

    // Covers: empty array -> empty Vec.
    #[test]
    fn parse_suggested_questions_empty_array_returns_empty() {
        let raw = r#"{"questions":[]}"#;
        let out = parse_suggested_questions_response(raw);
        assert!(out.is_empty(), "empty array must return empty Vec");
    }

    // Covers: empty strings dropped.
    #[test]
    fn parse_suggested_questions_drops_empty_strings() {
        let raw = r#"{"questions":["A","","B"]}"#;
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string(), "B".to_string()]);
    }

    // Covers: dedupe preserving first-seen order (contrast with parse_rewrite_response).
    #[test]
    fn parse_suggested_questions_dedupes_preserving_order() {
        let raw = r#"{"questions":["A","B","A"]}"#;
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string(), "B".to_string()]);
    }

    // Covers: truncates to POST_ANSWER_MAX_QUESTIONS (3).
    #[test]
    fn parse_suggested_questions_truncates_to_max() {
        let raw = r#"{"questions":["A","B","C","D","E"]}"#;
        let out = parse_suggested_questions_response(raw);
        assert_eq!(out, vec!["A".to_string(), "B".to_string(), "C".to_string()]);
    }

    // Covers: non-JSON -> empty Vec (NOT raw fallback; key diff from rewrite).
    #[test]
    fn parse_suggested_questions_non_json_returns_empty() {
        let raw = "not json at all";
        let out = parse_suggested_questions_response(raw);
        assert!(
            out.is_empty(),
            "non-JSON must return empty Vec, not raw fallback"
        );
    }

    // Covers: field is `questions`, not `queries` (guards against rewrite copy-paste).
    #[test]
    fn parse_suggested_questions_field_is_questions_not_queries() {
        let raw = r#"{"queries":["A"]}"#;
        let out = parse_suggested_questions_response(raw);
        assert!(out.is_empty(), "queries key must not be accepted");
    }

    // --- build_post_answer_prompt tests (US-CORE-037) ---

    // Covers: prompt grounds in user_message + answer + context (design §6.1).
    #[test]
    fn build_post_answer_prompt_includes_context_segments() {
        let user_message = "如何重置密码?";
        let assistant_text = "点击设置中的重置按钮。";
        let context_xml = "<context>KB-DOC-1</context>";
        let prompt = build_post_answer_prompt(user_message, assistant_text, context_xml);
        assert!(
            prompt.contains(user_message),
            "prompt must embed the user message"
        );
        assert!(
            prompt.contains(assistant_text),
            "prompt must embed the assistant answer"
        );
        assert!(
            prompt.contains(context_xml),
            "prompt must embed the retrieved context"
        );
        assert!(
            prompt.contains("questions"),
            "prompt must require the questions JSON key"
        );
    }

    // --- session_key tests (BE-D03) ---

    /// Covers: channel-scoped chat keys include channel id and session id with a prefix,
    /// so they cannot collide with old global sessions that used plain session_id.
    #[test]
    fn session_key_scopes_public_chat_by_channel() {
        let ch = ["help_center".to_string()];
        let key = session_key(Some(&ch), "sess-123");
        assert_eq!(key, "channel:help_center:sess-123");
    }

    /// Covers: scoped/internal chat uses a distinct prefix so it does not share
    /// the same key namespace as channel-scoped public chat.
    #[test]
    fn session_key_scopes_scoped_chat_without_channel() {
        let key = session_key(None, "sess-456");
        assert_eq!(key, "scoped:sess-456");
    }

    /// Covers: the same raw session id under different channels produces different
    /// storage keys, ensuring cross-channel session isolation.
    #[test]
    fn session_key_same_session_id_differs_by_channel() {
        let ch_a = ["channel_a".to_string()];
        let ch_b = ["channel_b".to_string()];
        let key_a = session_key(Some(&ch_a), "shared-session");
        let key_b = session_key(Some(&ch_b), "shared-session");
        assert_ne!(key_a, key_b, "same session id must be isolated by channel");
    }

    /// Covers: multi-channel requests join the sorted channel ids with a comma, so the
    /// same set of channels maps to one stable key regardless of input order.
    #[test]
    fn session_key_multi_channel_joins_sorted_ids() {
        let ch = ["help_center".to_string(), "dev_docs".to_string()];
        let key = session_key(Some(&ch), "sess-multi");
        assert_eq!(key, "channel:help_center,dev_docs:sess-multi");
    }

    #[tokio::test]
    async fn persist_round_interrupted_writes_user_and_flagged_partial_assistant() {
        let sessions: tokio::sync::Mutex<HashMap<String, rwiki_core::domain::chat::ChatSession>> =
            tokio::sync::Mutex::new(HashMap::new());
        persist_round(
            &sessions,
            "channel:ch:s1",
            "What is Rust?".to_string(),
            "Rust is a...".to_string(),
            true,
        )
        .await;

        let sessions = sessions.lock().await;
        let session = sessions.get("channel:ch:s1").expect("session created");
        assert_eq!(session.messages.len(), 2, "user + partial assistant");
        assert_eq!(session.messages[0].role, "user");
        assert_eq!(session.messages[0].content, "What is Rust?");
        assert!(!session.messages[0].interrupted);
        assert_eq!(session.messages[1].role, "assistant");
        assert_eq!(session.messages[1].content, "Rust is a...");
        assert!(
            session.messages[1].interrupted,
            "partial assistant answer must be flagged as interrupted"
        );
        assert!(
            !session.is_expired(rwiki_core::domain::chat::SESSION_TTL_SECS),
            "persisting refreshes last_accessed"
        );
    }

    #[tokio::test]
    async fn persist_round_interrupted_empty_partial_writes_user_only() {
        let sessions: tokio::sync::Mutex<HashMap<String, rwiki_core::domain::chat::ChatSession>> =
            tokio::sync::Mutex::new(HashMap::new());
        persist_round(
            &sessions,
            "channel:ch:s2",
            "Hello?".to_string(),
            String::new(),
            true,
        )
        .await;

        let sessions = sessions.lock().await;
        let session = sessions.get("channel:ch:s2").expect("session created");
        assert_eq!(
            session.messages.len(),
            1,
            "interrupted before first content chunk: no assistant message"
        );
        assert_eq!(session.messages[0].role, "user");
    }

    #[tokio::test]
    async fn persist_round_interrupted_appends_to_existing_session() {
        let sessions: tokio::sync::Mutex<HashMap<String, rwiki_core::domain::chat::ChatSession>> =
            tokio::sync::Mutex::new(HashMap::new());
        {
            let mut sessions = sessions.lock().await;
            let session = sessions
                .entry("channel:ch:s3".to_string())
                .or_insert_with_key(|id| rwiki_core::domain::chat::ChatSession::new(id.clone()));
            session.add_message("user", "earlier question");
            session.add_message("assistant", "earlier full answer");
        }
        persist_round(
            &sessions,
            "channel:ch:s3",
            "next question".to_string(),
            "partial".to_string(),
            true,
        )
        .await;

        let sessions = sessions.lock().await;
        let session = sessions.get("channel:ch:s3").expect("session exists");
        assert_eq!(
            session.messages.len(),
            4,
            "interrupted round appends instead of overwriting history"
        );
        assert_eq!(session.messages[2].content, "next question");
        assert!(session.messages[3].interrupted);
    }

    #[test]
    fn chat_request_deserializes_supports_content_end_event() {
        let req: ChatRequest = serde_json::from_str(
            r#"{"message":"hi","channelId":["ch"],"supportsContentEndEvent":true}"#,
        )
        .expect("new field accepted");
        assert!(req.supports_content_end_event);

        let legacy: ChatRequest = serde_json::from_str(r#"{"message":"hi","channelId":["ch"]}"#)
            .expect("legacy body without the field stays valid");
        assert!(
            !legacy.supports_content_end_event,
            "absent field must default to false so old clients never receive contentEnd"
        );
    }

    #[test]
    fn scoped_chat_request_deserializes_supports_content_end_event() {
        let req: ScopedChatRequest =
            serde_json::from_str(r#"{"message":"hi","supportsContentEndEvent":true}"#)
                .expect("new field accepted");
        assert!(req.supports_content_end_event);

        let legacy: ScopedChatRequest = serde_json::from_str(r#"{"message":"hi"}"#)
            .expect("legacy body without the field stays valid");
        assert!(!legacy.supports_content_end_event);
    }

    #[test]
    fn content_end_event_serializes_payload_with_discriminating_key() {
        // Real clients dispatch SSE events by payload shape, not event name;
        // an empty payload would be indistinguishable from `done`'s {}.
        let json = serde_json::to_string(&ContentEndEvent { content_end: true })
            .expect("serialization is infallible for a bool");
        assert_eq!(json, r#"{"contentEnd":true}"#);
    }
}
