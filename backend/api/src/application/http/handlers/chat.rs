use axum::{
    extract::{Query, State},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::StreamExt;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::streaming::StreamingChat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_stream::wrappers::ReceiverStream;
use tracing::Instrument;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use rwiki_core::domain::chat::{evict_expired_sessions, ChatMessage};
use rwiki_core::infrastructure::vector_store::SearchResult;

/// Rewrite LLM call timeout (ms)
const REWRITE_TIMEOUT_MS: u64 = 8000;
/// Max number of query variants from rewrite
const REWRITE_MAX_QUERIES: usize = 2;
/// RRF fusion parameter k
const RRF_K: u64 = 60;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatRequest {
    pub message: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct SuggestionsQuery {
    pub locale: Option<String>,
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

// ---------------------------------------------------------------------------
// Suggestions
// ---------------------------------------------------------------------------

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

/// Get suggested questions for a given locale.
#[utoipa::path(
    get,
    path = "/api/chat/suggestions",
    tag = "chat",
    params(SuggestionsQuery),
    responses(
        (status = 200, description = "Suggested questions", body = SuggestionsResponse)
    )
)]
pub async fn suggestions(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SuggestionsQuery>,
) -> Json<SuggestionsResponse> {
    let questions = match_locale(
        &state.chat_config.suggested_questions,
        params.locale.as_deref(),
    );
    Json(SuggestionsResponse { questions })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// 构建查询改写提示词。将对话历史与用户当前追问拼接，
/// 引导 LLM 将追问改写为独立的、自包含的查询。
pub(crate) fn build_rewrite_prompt(
    history: &[ChatMessage],
    user_message: &str,
    content_language: Option<&str>,
) -> String {
    let history_text = history
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n");
    let mut prompt = format!(
        "对话历史:\n{history_text}\n\n当前用户追问: {user_message}\n\n\
         请将用户的追问改写为一个独立的、自包含的查询。\n"
    );
    if let Some(lang) = content_language {
        if !lang.is_empty() {
            prompt.push_str(&format!(
                "知识库文档主要使用 {lang} 语言。请将查询改写为 {lang}，以确保检索能命中相关文档。\n"
            ));
        }
    }
    prompt.push_str(&format!(
        "输出严格的 JSON 格式：{{\"queries\": [\"改写1\", \"改写2\"]}}\n\
         最多生成 {REWRITE_MAX_QUERIES} 条查询变体。如果只有一个查询，也用数组包裹。\n\
         只输出 JSON，不要输出其他内容。"
    ));
    prompt
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

/// Build the first-turn rewrite prompt. Extends short/ambiguous queries
/// into more specific, searchable forms with JSON output constraint.
pub(crate) fn build_first_turn_rewrite_prompt(
    user_message: &str,
    content_language: Option<&str>,
) -> String {
    let mut prompt = format!(
        "用户查询: {user_message}\n\n\
         请将这个查询改写为更具体、更可检索的形式。\n\
         如果查询使用了非正式术语或缩写，替换为对应的正式术语。\n\
         如果查询包含多个独立的子问题，将每个子问题分别列出。\n"
    );
    if let Some(lang) = content_language {
        if !lang.is_empty() {
            prompt.push_str(&format!(
                "知识库文档主要使用 {lang} 语言。请将查询改写为 {lang}，以确保检索能命中相关文档。\n"
            ));
        }
    }
    prompt.push_str(&format!(
        "输出严格的 JSON 格式：{{\"queries\": [\"改写1\", \"改写2\"]}}\n\
         最多生成 {REWRITE_MAX_QUERIES} 条查询。如果只有一个查询，也用数组包裹。\n\
         只输出 JSON，不要输出其他内容。"
    ));
    prompt
}

/// Strip optional ```json ... ``` or ``` ... ``` fences from a raw LLM response.
fn strip_markdown_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.starts_with("```json") {
        trimmed
            .strip_prefix("```json")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```")
            .unwrap_or(trimmed)
            .strip_suffix("```")
            .unwrap_or(trimmed)
            .trim()
    } else {
        trimmed
    }
}

/// Parse the LLM rewrite response into a list of queries.
/// Three-level degradation:
///   1. Parse JSON {"queries": [...]} (with optional ```json fence stripping)
///   2. If JSON parse fails, use raw trimmed output as single query
///   3. Caller handles LLM failure by providing original query
pub(crate) fn parse_rewrite_response(raw: &str) -> Vec<String> {
    let json_str = strip_markdown_fences(raw);

    // 2. Try parsing JSON
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(arr) = val.get("queries").and_then(|v| v.as_array()) {
            let queries: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .take(REWRITE_MAX_QUERIES)
                .collect();
            if !queries.is_empty() {
                return queries;
            }
        }
    }

    // 3. Fallback: use entire output as single query
    tracing::debug!("rewrite response is not valid JSON, using raw output as single query");
    vec![raw.trim().to_string()]
}

fn preview_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Check whether raw response contains valid JSON with at least one non-empty query.
fn has_valid_rewrite_json(raw: &str) -> bool {
    let json_str = strip_markdown_fences(raw);
    serde_json::from_str::<serde_json::Value>(json_str)
        .ok()
        .and_then(|val| {
            val.get("queries").and_then(|v| v.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .any(|s| !s.trim().is_empty())
            })
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Chat with the knowledge base via SSE streaming.
///
/// Validates the request, searches the vector store for relevant context,
/// builds a per-request rig-core agent with the context injected into the
/// preamble, and streams the LLM response as SSE events.
#[utoipa::path(
    post,
    path = "/api/chat",
    tag = "chat",
    request_body = ChatRequest,
    responses(
        (status = 200, description = "SSE stream of chat response events", content_type = "text/event-stream"),
        (status = 400, description = "Message cannot be empty", body = ErrorResponse),
        (status = 503, description = "Knowledge base is empty", body = ErrorResponse)
    )
)]
pub async fn chat(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // Validate message is not empty
    if req.message.trim().is_empty() {
        return Err(ApiError::bad_request("消息不能为空"));
    }

    // Check knowledge base is not empty
    if state.vector_store.is_empty().await {
        return Err(ApiError::service_unavailable(
            "当前知识库中没有索引数据，请先上传文档",
        ));
    }

    // Determine session ID
    let is_new_session = req.session_id.is_none();
    let session_id = req.session_id.unwrap_or_else(|| Uuid::now_v7().to_string());
    let chat_span = tracing::info_span!(
        "chat_request",
        session_id = %session_id,
        user_message_preview = %preview_chars(&req.message, 50),
        user_message_len = req.message.chars().count(),
        is_new_session,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
    );

    async move {

    // Get session state: summary + history (with eviction + touch), then release lock
    let (summary, history) = {
        let mut sessions = state.chat_sessions.lock().await;
        evict_expired_sessions(&mut sessions);
        if let Some(session) = sessions.get_mut(&session_id) {
            session.touch();
            (session.summary.clone(), session.messages.clone())
        } else {
            (None, Vec::new())
        }
    };

    // Query rewriting: always rewrite (unconditional)
    let search_queries = {
        let query_rewrite_span = tracing::info_span!(
            "query_rewrite",
            original_query_preview = %preview_chars(&req.message, 50),
            original_query_len = req.message.chars().count(),
            is_first_turn = history.is_empty(),
            timed_out = tracing::field::Empty,
            fallback_reason = tracing::field::Empty,
            rewrite_count = tracing::field::Empty,
            rewritten_queries_preview = tracing::field::Empty,
            error = tracing::field::Empty,
            error.message = tracing::field::Empty,
        );

        async {
        let content_language = state
            .chat_config
            .content_language
            .as_deref()
            .filter(|s| !s.is_empty());
        let (rewrite_preamble, rewrite_prompt) =
            if history.is_empty() {
                (
                    "You are a query rewriting assistant. Expand short or vague queries into more specific, retrievable forms.",
                    build_first_turn_rewrite_prompt(&req.message, content_language),
                )
            } else {
                ("You are a query rewriting assistant. Based on conversation history, rewrite the user's follow-up into a standalone, self-contained query.",
             build_rewrite_prompt(&history, &req.message, content_language))
            };

        let rewrite_agent = state
            .llm_client
            .agent(&state.llm_model)
            .preamble(rewrite_preamble)
            .max_tokens(200)
            .build();

        let current_span = tracing::Span::current();
        let queries = match tokio::time::timeout(
            Duration::from_millis(REWRITE_TIMEOUT_MS),
            rewrite_agent.prompt(&rewrite_prompt),
        )
        .await
        {
            Ok(Ok(raw_response)) => {
                current_span.record("timed_out", false);
                let queries = parse_rewrite_response(&raw_response);
                let json_str = strip_markdown_fences(&raw_response);
                let fallback_reason = if has_valid_rewrite_json(&raw_response) {
                    "none"
                } else if serde_json::from_str::<serde_json::Value>(json_str).is_ok() {
                    "empty_queries"
                } else {
                    "invalid_json"
                };
                current_span.record("fallback_reason", fallback_reason);
                queries
            }
            Ok(Err(e)) => {
                tracing::warn!("query rewriting failed: {e}, falling back to original query");
                current_span.record("timed_out", false);
                current_span.record("fallback_reason", "llm_error");
                current_span.record("error.message", e.to_string());
                vec![req.message.clone()]
            }
            Err(_) => {
                tracing::warn!(
                    "query rewriting timed out after {REWRITE_TIMEOUT_MS}ms, falling back to original query"
                );
                current_span.record("timed_out", true);
                current_span.record("fallback_reason", "timeout");
                current_span.record(
                    "error.message",
                    format!("query rewriting timed out after {REWRITE_TIMEOUT_MS}ms"),
                );
                vec![req.message.clone()]
            }
        };
        current_span.record("rewrite_count", queries.len());
        current_span.record(
            "rewritten_queries_preview",
            preview_chars(&queries.join(", "), 200),
        );
        queries
        }
        .instrument(query_rewrite_span)
        .await
    };

    // Hybrid search with keyword + vector fusion
    tracing::debug!("search queries: {:?}", search_queries);
    let chat_request_span = tracing::Span::current();
    let retrieval_span = tracing::info_span!(
        "retrieval",
        query_count = search_queries.len(),
        total_results = tracing::field::Empty,
        top_scores = tracing::field::Empty,
        result_titles = tracing::field::Empty,
        error = tracing::field::Empty,
        error.message = tracing::field::Empty,
    );
    let search_results = async {
        let current_span = tracing::Span::current();
        let results = if search_queries.len() == 1 {
            // Single query: direct hybrid search
            state
                .vector_store
                .search_hybrid(&search_queries[0], 5, 1, 3, 12, RRF_K)
                .await
        } else {
            // Multi-query: hybrid search per query + RRF fusion
            let results = state
                .vector_store
                .search_multi_query_hybrid(&search_queries, 5, 1, 3, 12, RRF_K)
                .await?;
            // Fallback: if all rewrite queries returned empty, retry with original query
            if results.is_empty() {
                tracing::warn!("all rewrite queries returned empty, falling back to original query");
                state
                    .vector_store
                    .search_hybrid(&req.message, 5, 1, 3, 12, RRF_K)
                    .await
            } else {
                Ok(results)
            }
        };

        match results {
            Ok(results) => {
                current_span.record("total_results", results.len());
                current_span.record(
                    "top_scores",
                    results
                        .iter()
                        .take(5)
                        .map(|r| format!("{:.4}", r.score))
                        .collect::<Vec<_>>()
                        .join(", "),
                );
                current_span.record(
                    "result_titles",
                    preview_chars(
                        &results
                            .iter()
                            .take(5)
                            .map(|r| r.title.as_str())
                            .collect::<Vec<_>>()
                            .join(", "),
                        200,
                    ),
                );
                Ok(results)
            }
            Err(e) => {
                current_span.record("error", true);
                current_span.record("error.message", e.to_string());
                chat_request_span.record("error", true);
                chat_request_span.record("error.message", e.to_string());
                Err(ApiError::internal(e.to_string()))
            }
        }
    }
    .instrument(retrieval_span)
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

    // Rerank: re-score candidates via cross-encoder API when enabled
    let search_results = if let Some(reranker) = &state.reranker {
        let truncated: Vec<&SearchResult> = search_results.iter()
            .take(state.rerank_config.top_n)
            .collect();
        let documents: Vec<String> = truncated.iter().map(|r| r.content.clone()).collect();

        match reranker.rerank(&req.message, &documents, state.rerank_config.top_n).await {
            Ok(rerank_results) => {
                tracing::debug!("rerank returned {} results", rerank_results.len());
                rerank_results
                    .into_iter()
                    .filter_map(|rr| truncated.get(rr.index).map(|r| (r, rr.relevance_score)))
                    .map(|(r, score)| {
                        let mut result = (*r).clone();
                        result.score = score;
                        result
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!("Rerank failed, degrading to RRF fusion results: {e}");
                search_results
            }
        }
    } else {
        search_results
    };

    let context_text = format!(
        "<documents>\n{}\n</documents>",
        search_results
            .iter()
            .enumerate()
            .map(|(i, r)| format_context_block(i + 1, r))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let context_chunks = search_results.len();
    let context_chars = context_text.chars().count();

    // Build preamble with system_prompt + optional summary + RAG context
    let preamble = build_preamble(
        &state.chat_config.system_prompt,
        summary.as_deref(),
        &context_text,
    );

    // Build per-request agent with context in preamble
    let agent = state
        .llm_client
        .agent(&state.llm_model)
        .preamble(&preamble)
        .build();

    // Build chat history from sliding window only
    let sliding_window_size = state.chat_config.sliding_window_size;
    let sliding_window = {
        let sessions = state.chat_sessions.lock().await;
        if let Some(session) = sessions.get(&session_id) {
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
                rig::completion::Message::from(rig::message::AssistantContent::text(&msg.content))
            }
        })
        .collect();

    let user_message = req.message.clone();

    // Extract config values before spawn (state moves into the closure)
    let compact_threshold = state.chat_config.compact_threshold;
    let token_budget = state.chat_config.token_budget;
    let llm_model_for_span = state.llm_model.clone();

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
            return;
        }

        // Stream LLM response using stream_chat (with original user message)
        let llm_started_at = Instant::now();
        let mut first_text_chunk_seen = false;
        let mut stream = agent.stream_chat(user_message.clone(), chat_history).await;
        let mut assistant_text = String::new();

        while let Some(item) = stream.next().await {
            match item {
                Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                    rig::streaming::StreamedAssistantContent::Text(rig::message::Text {
                        text, ..
                    }),
                )) => {
                    if !first_text_chunk_seen {
                        first_text_chunk_seen = true;
                        tracing::Span::current().record(
                            "first_token_latency_ms",
                            llm_started_at.elapsed().as_millis() as u64,
                        );
                    }
                    assistant_text.push_str(&text);
                    let chunk_event = ChunkEvent { content: text };
                    let event = Event::default()
                        .event("chunk")
                        .data(serde_json::to_string(&chunk_event).unwrap_or_default());
                    if tx.send(Ok(event)).await.is_err() {
                        tracing::Span::current()
                            .record("output_chars", assistant_text.chars().count());
                        return;
                    }
                }
                Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)) => {
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
                    let error_event = ErrorEvent {
                        message: "Failed to generate response. Please try again later.".to_string(),
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

        // Persist user message and assistant response to session
        {
            let mut sessions = state.chat_sessions.lock().await;
            let session = sessions
                .entry(session_id.clone())
                .or_insert_with_key(|id| rwiki_core::domain::chat::ChatSession::new(id.clone()));
            session.add_message("user", &user_message);
            if !assistant_text.is_empty() {
                session.add_message("assistant", &assistant_text);
            }
        }

        // Compact check: if session exceeds thresholds, compress old messages
        let sessions = state.chat_sessions.clone();
        let llm_client = state.llm_client.clone();
        let llm_model = state.llm_model.clone();
        {
            let mut sessions_lock = sessions.lock().await;
            if let Some(session) = sessions_lock.get_mut(&session_id) {
                if session.should_compact(compact_threshold, token_budget, sliding_window_size) {
                    let old_count = session.messages.len().saturating_sub(sliding_window_size);
                    let old_messages = session.messages[..old_count].to_vec();
                    let existing_summary = session.summary.clone();
                    drop(sessions_lock); // release lock before LLM call

                    let prompt = build_compact_prompt(existing_summary.as_deref(), &old_messages);
                    let compact_agent = llm_client
                        .agent(&llm_model)
                        .preamble(
                            "请将以下对话历史压缩为一段简洁的摘要，保留关键信息、事实和上下文。",
                        )
                        .max_tokens(300)
                        .build();
                    match compact_agent.prompt(&prompt).await {
                        Ok(new_summary) => {
                            if let Some(session) = sessions.lock().await.get_mut(&session_id) {
                                session.compact_history(new_summary, sliding_window_size);
                            } else {
                                tracing::warn!(
                                    "session {session_id} evicted during compact, skipping"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!("compact failed: {e}, keeping original messages");
                        }
                    }
                }
            }
        }
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

    // --- build_rewrite_prompt tests ---

    #[test]
    fn build_rewrite_prompt_includes_history_and_user_message() {
        let history = vec![
            ChatMessage {
                role: "user".into(),
                content: "What is Rust?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Rust is a systems language.".into(),
            },
        ];
        let prompt = build_rewrite_prompt(&history, "How does it handle memory?", None);
        assert!(
            prompt.contains("user: What is Rust?"),
            "should include user history"
        );
        assert!(
            prompt.contains("assistant: Rust is a systems language."),
            "should include assistant history"
        );
        assert!(
            prompt.contains("当前用户追问: How does it handle memory?"),
            "should include the user message"
        );
        assert!(prompt.contains("改写"), "should instruct rewriting");
    }

    #[test]
    fn build_rewrite_prompt_with_empty_history_produces_valid_prompt() {
        let history: Vec<ChatMessage> = vec![];
        let prompt = build_rewrite_prompt(&history, "What is Rust?", None);
        assert!(
            prompt.contains("当前用户追问: What is Rust?"),
            "should include user message even with empty history"
        );
        assert!(
            prompt.contains("对话历史:\n\n"),
            "should have empty history section"
        );
    }

    // --- build_compact_prompt tests ---

    #[test]
    fn build_compact_prompt_with_existing_summary_includes_old_summary() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "What is Rust?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "A systems language.".into(),
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

    // --- build_first_turn_rewrite_prompt tests ---

    #[test]
    fn build_first_turn_rewrite_prompt_includes_user_message() {
        let prompt = build_first_turn_rewrite_prompt("内存", None);
        assert!(
            prompt.contains("用户查询: 内存"),
            "should include user message"
        );
        assert!(
            prompt.contains("JSON"),
            "should include JSON format constraint"
        );
        assert!(
            prompt.contains(&format!("最多生成 {REWRITE_MAX_QUERIES} 条查询")),
            "should reference REWRITE_MAX_QUERIES constant"
        );
    }

    #[test]
    fn build_first_turn_rewrite_prompt_references_max_queries_constant() {
        let prompt = build_first_turn_rewrite_prompt("test", None);
        assert!(
            prompt.contains("最多生成 2 条查询"),
            "should embed the REWRITE_MAX_QUERIES value"
        );
    }

    // --- parse_rewrite_response tests ---

    #[test]
    fn parse_rewrite_response_valid_json() {
        let result = parse_rewrite_response(r#"{"queries": ["query one", "query two"]}"#);
        assert_eq!(result, vec!["query one", "query two"]);
    }

    #[test]
    fn parse_rewrite_response_json_with_code_fence() {
        let raw = "```json\n{\"queries\": [\"q1\", \"q2\"]}\n```";
        let result = parse_rewrite_response(raw);
        assert_eq!(result, vec!["q1", "q2"]);
    }

    #[test]
    fn parse_rewrite_response_plain_code_fence() {
        let raw = "```\n{\"queries\": [\"q1\"]}\n```";
        let result = parse_rewrite_response(raw);
        assert_eq!(result, vec!["q1"]);
    }

    #[test]
    fn parse_rewrite_response_empty_queries_array_falls_back() {
        let result = parse_rewrite_response("{\"queries\": []}");
        assert_eq!(
            result.len(),
            1,
            "empty array should fall back to raw output"
        );
        assert_eq!(result[0], "{\"queries\": []}");
    }

    #[test]
    fn parse_rewrite_response_missing_queries_field_falls_back() {
        let result = parse_rewrite_response("{\"result\": \"something\"}");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "{\"result\": \"something\"}");
    }

    #[test]
    fn parse_rewrite_response_plain_text_falls_back() {
        let result = parse_rewrite_response("This is just plain text");
        assert_eq!(result, vec!["This is just plain text"]);
    }

    #[test]
    fn parse_rewrite_response_truncates_to_max_queries() {
        let result = parse_rewrite_response(r#"{"queries": ["q1", "q2", "q3", "q4"]}"#);
        assert_eq!(
            result.len(),
            REWRITE_MAX_QUERIES,
            "should truncate to REWRITE_MAX_QUERIES"
        );
    }

    #[test]
    fn parse_rewrite_response_filters_empty_strings() {
        let result = parse_rewrite_response(r#"{"queries": ["q1", "", "  ", "q2"]}"#);
        assert_eq!(result, vec!["q1", "q2"]);
    }

    #[test]
    fn parse_rewrite_response_single_query_in_array() {
        let result = parse_rewrite_response("{\"queries\": [\"single query\"]}");
        assert_eq!(result, vec!["single query"]);
    }

    // --- modified build_rewrite_prompt JSON constraint test ---

    #[test]
    fn build_rewrite_prompt_includes_json_format_constraint() {
        let history = vec![
            ChatMessage {
                role: "user".into(),
                content: "What is Rust?".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Rust is a systems language.".into(),
            },
        ];
        let prompt = build_rewrite_prompt(&history, "How does it handle memory?", None);
        assert!(
            prompt.contains("JSON"),
            "modified prompt should include JSON format constraint"
        );
        assert!(
            prompt.contains("queries"),
            "modified prompt should reference queries array"
        );
        assert!(
            prompt.contains("How does it handle memory?"),
            "should still include user message"
        );
    }

    // --- content_language injection tests ---

    #[test]
    fn build_first_turn_rewrite_prompt_includes_language_instruction_when_set() {
        let prompt = build_first_turn_rewrite_prompt("memory management", Some("Chinese"));
        assert!(
            prompt.contains("知识库文档主要使用 Chinese 语言"),
            "should include language instruction when content_language is set"
        );
        assert!(
            prompt.contains("请将查询改写为 Chinese"),
            "should instruct rewriting to target language"
        );
    }

    #[test]
    fn build_rewrite_prompt_includes_language_instruction_when_set() {
        let history = vec![ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let prompt = build_rewrite_prompt(&history, "follow up", Some("English"));
        assert!(
            prompt.contains("知识库文档主要使用 English 语言"),
            "should include language instruction when content_language is set"
        );
    }

    #[test]
    fn build_first_turn_rewrite_prompt_no_language_instruction_when_none() {
        let prompt = build_first_turn_rewrite_prompt("test", None);
        assert!(
            !prompt.contains("知识库文档主要使用"),
            "should NOT include language instruction when content_language is None"
        );
    }

    #[test]
    fn build_rewrite_prompt_no_language_instruction_when_none() {
        let history = vec![ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let prompt = build_rewrite_prompt(&history, "test", None);
        assert!(
            !prompt.contains("知识库文档主要使用"),
            "should NOT include language instruction when content_language is None"
        );
    }

    // --- content_language edge-case prompt tests (BE-T01) ---

    // Covers: Design 5.1 — empty string content_language suppresses language instruction.
    // User Story: Query language aware rewrite — empty string is treated as "no language" by prompt builders.
    #[test]
    fn build_first_turn_rewrite_prompt_empty_string_no_language_instruction() {
        let prompt = build_first_turn_rewrite_prompt("test query", Some(""));
        assert!(
            !prompt.contains("知识库文档主要使用"),
            "empty string content_language should NOT produce language instruction"
        );
    }

    // Covers: Design 5.1 — empty string content_language suppresses language instruction in multi-turn.
    // User Story: Query language aware rewrite — empty string treated as absent in rewrite prompt.
    #[test]
    fn build_rewrite_prompt_empty_string_no_language_instruction() {
        let history = vec![ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let prompt = build_rewrite_prompt(&history, "follow up", Some(""));
        assert!(
            !prompt.contains("知识库文档主要使用"),
            "empty string content_language should NOT produce language instruction"
        );
    }

    // Covers: Design 5.1 — Chinese language value produces correct instruction text.
    // User Story: Query language aware rewrite — Chinese content_language injects both context and rewrite instruction.
    #[test]
    fn build_first_turn_rewrite_prompt_chinese_language_value() {
        let prompt = build_first_turn_rewrite_prompt("memory management", Some("中文"));
        assert!(
            prompt.contains("知识库文档主要使用 中文 语言"),
            "should include Chinese language instruction with Chinese value"
        );
        assert!(
            prompt.contains("请将查询改写为 中文"),
            "should instruct rewriting to 中文"
        );
    }

    // Covers: Design 5.1 — Chinese language value in multi-turn prompt.
    // User Story: Query language aware rewrite — Chinese content_language works in follow-up rewrite.
    #[test]
    fn build_rewrite_prompt_chinese_language_value() {
        let history = vec![ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let prompt = build_rewrite_prompt(&history, "follow up", Some("中文"));
        assert!(
            prompt.contains("知识库文档主要使用 中文 语言"),
            "should include Chinese language instruction in multi-turn prompt"
        );
    }

    // Covers: Design 5.1 — language instruction appears before JSON constraint in first-turn prompt.
    // User Story: Query language aware rewrite — prompt ordering ensures LLM sees language context before output format.
    #[test]
    fn build_first_turn_rewrite_prompt_language_before_json_constraint() {
        let prompt = build_first_turn_rewrite_prompt("test", Some("English"));
        let lang_pos = prompt
            .find("知识库文档主要使用")
            .expect("should contain language instruction");
        let json_pos = prompt
            .find("只输出 JSON")
            .expect("should contain JSON constraint");
        assert!(
            lang_pos < json_pos,
            "language instruction should appear BEFORE JSON constraint"
        );
    }

    // Covers: Design 5.1 — language instruction appears before JSON constraint in multi-turn prompt.
    // User Story: Query language aware rewrite — prompt ordering consistent across first-turn and follow-up.
    #[test]
    fn build_rewrite_prompt_language_before_json_constraint() {
        let history = vec![ChatMessage {
            role: "user".into(),
            content: "test".into(),
        }];
        let prompt = build_rewrite_prompt(&history, "follow up", Some("English"));
        let lang_pos = prompt
            .find("知识库文档主要使用")
            .expect("should contain language instruction");
        let json_pos = prompt
            .find("只输出 JSON")
            .expect("should contain JSON constraint");
        assert!(
            lang_pos < json_pos,
            "language instruction should appear BEFORE JSON constraint in multi-turn prompt"
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
}
