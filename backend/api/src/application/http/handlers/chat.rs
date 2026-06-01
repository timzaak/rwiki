use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::StreamExt;
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::streaming::StreamingChat;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;
use rwiki_core::domain::chat::{evict_expired_sessions, ChatMessage};
use rwiki_core::infrastructure::vector_store::SearchResult;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
pub struct ChatRequest {
    pub message: String,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
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
        parts.push(format!("\n对话摘要:\n{s}"));
    }
    parts.push(format!("\n上下文:\n{rag_context}"));
    parts.join("\n")
}

/// 构建查询改写提示词。将对话历史与用户当前追问拼接，
/// 引导 LLM 将追问改写为独立的、自包含的查询。
pub(crate) fn build_rewrite_prompt(history: &[ChatMessage], user_message: &str) -> String {
    let history_text = history
        .iter()
        .map(|msg| format!("{}: {}", msg.role, msg.content))
        .collect::<Vec<_>>()
        .join("\n");
    format!("对话历史:\n{history_text}\n\n当前用户追问: {user_message}\n\n请将用户的追问改写为一个独立的、自包含的查询，使其在不依赖对话历史的情况下也能被理解。")
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
    let session_id = req.session_id.unwrap_or_else(|| Uuid::now_v7().to_string());

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

    // Query rewriting: if history is non-empty, rewrite the user query for RAG
    let search_query = if history.is_empty() {
        req.message.clone()
    } else {
        let rewrite_prompt = build_rewrite_prompt(&history, &req.message);
        let rewrite_agent = state
            .llm_client
            .agent(&state.llm_model)
            .preamble("你是一个查询改写助手。根据对话历史，将用户当前的追问改写为一个独立的、自包含的查询。")
            .max_tokens(100)
            .build();
        match rewrite_agent.prompt(&rewrite_prompt).await {
            Ok(rewritten) => rewritten,
            Err(e) => {
                tracing::warn!("query rewriting failed: {e}, falling back to original query");
                req.message.clone()
            }
        }
    };

    // Search vector store for relevant context with window expansion
    let search_results = state
        .vector_store
        .search_with_expansion(&search_query, 5, 1, 3, 12)
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let context_text = search_results
        .iter()
        .map(format_context_block)
        .collect::<Vec<_>>()
        .join("\n\n");

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

    // Spawn a task to stream LLM response
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Event, Infallible>>(32);

    tokio::spawn(async move {
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
        let mut stream = agent.stream_chat(user_message.clone(), chat_history).await;
        let mut assistant_text = String::new();

        while let Some(item) = stream.next().await {
            match item {
                Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(
                    rig::streaming::StreamedAssistantContent::Text(rig::message::Text { text }),
                )) => {
                    assistant_text.push_str(&text);
                    let chunk_event = ChunkEvent { content: text };
                    let event = Event::default()
                        .event("chunk")
                        .data(serde_json::to_string(&chunk_event).unwrap_or_default());
                    if tx.send(Ok(event)).await.is_err() {
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
                    let error_event = ErrorEvent {
                        message: "回答生成失败，请稍后重试".to_string(),
                    };
                    let event = Event::default()
                        .event("error")
                        .data(serde_json::to_string(&error_event).unwrap_or_default());
                    let _ = tx.send(Ok(event)).await;
                    return;
                }
            }
        }

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
    });

    let stream = ReceiverStream::new(rx);
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_context_block(result: &SearchResult) -> String {
    let mut lines = Vec::new();

    // Source line: Title / Section
    let source = match &result.section {
        Some(s) if !s.is_empty() => format!("{} / {}", result.title, s),
        _ => result.title.clone(),
    };
    lines.push(format!("[Source: {source}]"));

    // Link line (only if present and non-empty)
    if let Some(link) = &result.link {
        if !link.is_empty() {
            lines.push(format!("Link: {link}"));
        }
    }

    // Locale line (only if present and non-empty)
    if let Some(locale) = &result.locale {
        if !locale.is_empty() {
            lines.push(format!("Locale: {locale}"));
        }
    }

    // Content
    lines.push(result.content.clone());

    lines.join("\n")
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
        let output = format_context_block(&result);
        assert!(
            output.contains("[Source: Getting Started / Installation]"),
            "should contain Source line with title and section"
        );
        assert!(
            output.contains("Link: https://example.com/docs"),
            "should contain Link line"
        );
        assert!(
            output.contains("Locale: zh-CN"),
            "should contain Locale line"
        );
        assert!(
            output.contains("Install via cargo."),
            "should contain content"
        );
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
        let output = format_context_block(&result);
        assert!(
            output.contains("[Source: Getting Started / Installation]"),
            "should contain Source line"
        );
        assert!(
            !output.contains("Link:"),
            "should NOT contain Link line when link is None"
        );
        assert!(
            !output.contains("Locale:"),
            "should NOT contain Locale line when locale is None"
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
        let output = format_context_block(&result);
        assert_eq!(
            output.lines().next().unwrap(),
            "[Source: Getting Started]",
            "should show title only when no section"
        );
        assert!(
            output.contains("Link: https://example.com/docs"),
            "should contain Link line"
        );
    }

    #[test]
    fn format_context_block_no_section_no_link() {
        let result = make_result("Getting Started", None, None, None, "Content here.");
        let output = format_context_block(&result);
        assert_eq!(
            output.lines().next().unwrap(),
            "[Source: Getting Started]",
            "should show title only"
        );
        assert!(!output.contains("Link:"), "should NOT contain Link line");
        assert!(
            !output.contains("Locale:"),
            "should NOT contain Locale line"
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
        let output = format_context_block(&result);
        assert!(
            !output.contains("Link:"),
            "empty string link should NOT produce Link line"
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
        let output = format_context_block(&result);
        assert!(
            !output.contains("Locale:"),
            "empty string locale should NOT produce Locale line"
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

        let blocks: Vec<String> = results.iter().map(format_context_block).collect();

        // First result: has link and locale
        assert!(
            blocks[0].contains("Link: https://a.com"),
            "first result should have Link line"
        );
        assert!(
            blocks[0].contains("Locale: en"),
            "first result should have Locale line"
        );

        // Second result: no link, no locale
        assert!(
            !blocks[1].contains("Link:"),
            "second result should NOT have Link line"
        );
        assert!(
            !blocks[1].contains("Locale:"),
            "second result should NOT have Locale line"
        );

        // Third result: has link but no locale
        assert!(
            blocks[2].contains("Link: https://c.com"),
            "third result should have Link line"
        );
        assert!(
            !blocks[2].contains("Locale:"),
            "third result should NOT have Locale line"
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
            preamble.contains("上下文:\nRAG context."),
            "should contain RAG context"
        );
        assert!(
            !preamble.contains("对话摘要"),
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
            preamble.contains("对话摘要:\nPreviously discussed Rust generics."),
            "should include summary between system_prompt and RAG context"
        );
        assert!(
            preamble.contains("System prompt."),
            "should contain system prompt"
        );
        assert!(
            preamble.contains("上下文:\nRAG context."),
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
            preamble.contains("上下文:\nSome context."),
            "preamble should preserve context even when system_prompt is empty"
        );
    }

    #[test]
    fn build_preamble_with_multiline_system_prompt_preserves_separator() {
        let system_prompt = "Line one.\nLine two.\nLine three.";
        let preamble = build_preamble(system_prompt, None, "Context.");
        assert!(
            preamble.contains("上下文:\nContext."),
            "separator must exist even with multi-line system_prompt"
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
        let prompt = build_rewrite_prompt(&history, "How does it handle memory?");
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
        let prompt = build_rewrite_prompt(&history, "What is Rust?");
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
}
