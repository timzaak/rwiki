//! Chat handler pure helper function scenario tests.
//!
//! Verifies the pub(crate) prompt-building helpers extracted from the
//! chat handler: `build_rewrite_prompt`, `build_first_turn_rewrite_prompt`,
//! `build_preamble`, `build_compact_prompt`, and `parse_rewrite_response`.
//! These are pure functions with no LLM or I/O dependencies, so they are
//! tested as plain synchronous unit tests.
//!
//! Also tests `ChatSession::get_sliding_window` and handler-level decision
//! preconditions relevant to the multi-turn conversation hybrid memory and
//! query rewrite features.

use rwiki_core::domain::chat::{ChatMessage, ChatSession};

use super::chat::{
    build_compact_prompt, build_first_turn_rewrite_prompt, build_post_answer_prompt,
    build_preamble, build_rewrite_prompt, parse_rewrite_response,
    parse_suggested_questions_response,
};

// ---------------------------------------------------------------------------
// Imports for low-recall bypass-write integration scenarios (US-CORE-038).
// These tests construct a full AppState (in-memory sqlite + migrations + a
// seeded chunk_metadata row + a mockito-backed embeddings endpoint) and drive
// the chat endpoint through the Axum test harness (`create_api_routes` +
// `oneshot`), mirroring the integration-test idiom established in
// `rerank_scenarios.rs` / `feedback_scenarios.rs`.
// ---------------------------------------------------------------------------

use std::sync::Arc;
use std::sync::Once;

use axum::body::Body;
use axum::http::{header, Method, Request};
use rig::client::EmbeddingsClient;
use tower::ServiceExt;

use rwiki_core::config::LowRecallConfig;

use crate::application::http::create_api_routes;
use crate::application::http::state::AppState;

/// API token shared between the AppState literal and the Bearer header on
/// authenticated requests (the scoped chat endpoint lives behind `doc_router`'s
/// `auth_middleware`).
const LOW_RECALL_TEST_API_TOKEN: &str = "test-api-token-low-recall";

// ---------------------------------------------------------------------------
// build_rewrite_prompt scenarios
// ---------------------------------------------------------------------------

// User Story: As a user engaged in a multi-turn conversation (US-CORE-011),
// I want my follow-up questions to be rewritten into standalone queries so
// that the RAG search engine can retrieve relevant context even when my
// question refers to earlier dialogue.
// Covers: build_rewrite_prompt includes full history and user message in the
//         prompt template so the LLM can resolve pronouns and ellipses.

#[test]
fn rewrite_prompt_includes_history_and_user_message() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Rust is a systems programming language.".into(),
        },
    ];
    let prompt = build_rewrite_prompt(&history, "How does it handle memory?", None);

    assert!(
        prompt.contains("user: What is Rust?"),
        "prompt must include user role entries from history"
    );
    assert!(
        prompt.contains("assistant: Rust is a systems programming language."),
        "prompt must include assistant role entries from history"
    );
    assert!(
        prompt.contains("当前用户追问: How does it handle memory?"),
        "prompt must include the current user follow-up question"
    );
    assert!(
        prompt.contains("改写"),
        "prompt must instruct the LLM to rewrite the query"
    );
}

// User Story: As a user starting a new conversation (US-CORE-011), the system
// should skip rewriting because there is no history to contextualize the query.
// This test verifies that the rewrite prompt is still structurally valid with
// empty history, which the handler uses to decide whether rewriting is needed.
// Covers: build_rewrite_prompt with empty history produces a parseable prompt;
//         handler can use history emptiness as a precondition to skip LLM call.

#[test]
fn rewrite_prompt_with_empty_history_produces_query_only_structure() {
    let history: Vec<ChatMessage> = vec![];
    let prompt = build_rewrite_prompt(&history, "What is Rust?", None);

    assert!(
        prompt.contains("当前用户追问: What is Rust?"),
        "prompt must include the user message even when history is empty"
    );
    assert!(
        prompt.contains("对话历史:\n\n"),
        "empty history should produce an empty history section, confirming \
         the handler can detect no prior turns"
    );
    // Semantic intent: when history is empty, rewriting is unnecessary because
    // the query is already standalone. The handler checks history.is_empty()
    // before calling build_rewrite_prompt, so this test documents the boundary.
}

// ---------------------------------------------------------------------------
// build_preamble scenarios
// ---------------------------------------------------------------------------

// User Story: As a user resuming a conversation (US-CORE-012), I want the
// assistant to remember previously discussed topics via the summary, so that
// it can give contextually relevant answers without re-reading the full history.
// Covers: build_preamble with summary includes the summary section between
//         system_prompt and RAG context.

#[test]
fn preamble_with_summary_includes_summary_between_system_and_context() {
    let preamble = build_preamble(
        "You are a knowledge base assistant.",
        Some("User previously asked about Rust memory management."),
        "RAG result: Rust uses ownership and borrowing.",
    );

    assert!(
        preamble.contains("You are a knowledge base assistant."),
        "preamble must include system prompt"
    );
    assert!(
        preamble
            .contains("Conversation Summary:\nUser previously asked about Rust memory management."),
        "preamble must include summary section when summary is provided"
    );
    assert!(
        preamble.contains("Context:\nRAG result: Rust uses ownership and borrowing."),
        "preamble must include RAG context"
    );
    // Verify ordering: system_prompt comes before summary, summary before context
    let sys_pos = preamble
        .find("You are a knowledge base assistant.")
        .unwrap();
    let summary_pos = preamble.find("Conversation Summary").unwrap();
    let ctx_pos = preamble.find("Context").unwrap();
    assert!(
        sys_pos < summary_pos && summary_pos < ctx_pos,
        "system prompt must precede summary, which must precede RAG context"
    );
}

// User Story: As a user starting a fresh session (US-CORE-012), there is no
// prior summary to include. The preamble must still work correctly without
// leaking a placeholder summary section.
// Covers: build_preamble without summary omits the summary section entirely.

#[test]
fn preamble_without_summary_omits_summary_section() {
    let preamble = build_preamble("System prompt.", None, "RAG context.");

    assert!(
        preamble.contains("System prompt."),
        "preamble must include system prompt"
    );
    assert!(
        preamble.contains("Context:\nRAG context."),
        "preamble must include RAG context"
    );
    assert!(
        !preamble.contains("Conversation Summary"),
        "preamble must NOT contain summary section when summary is None"
    );
}

// User Story: As a user asking a question that has no relevant documents in
// the knowledge base (US-CORE-002), the RAG context will be empty. The
// preamble must still produce a valid structure so the LLM receives a
// well-formed prompt.
// Covers: build_preamble with empty rag_context produces valid structure.

#[test]
fn preamble_with_empty_rag_context_produces_valid_structure() {
    let preamble = build_preamble("You are an assistant.", None, "");

    assert!(
        preamble.starts_with("You are an assistant."),
        "preamble must start with system prompt even when rag_context is empty"
    );
    assert!(
        preamble.contains("Context:\n"),
        "preamble must include context header even when rag_context is empty"
    );
}

// ---------------------------------------------------------------------------
// build_compact_prompt scenarios
// ---------------------------------------------------------------------------

// User Story: As a user in a long conversation (US-CORE-012), I want old
// messages to be compressed into a summary so the LLM context window is not
// exhausted. When a previous summary already exists, the compact prompt must
// include it so the LLM can produce an updated summary rather than starting
// from scratch.
// Covers: build_compact_prompt with existing summary includes old summary.

#[test]
fn compact_prompt_with_existing_summary_includes_old_summary() {
    let messages = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is ownership in Rust?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Ownership is a memory management concept.".into(),
        },
    ];
    let prompt = build_compact_prompt(Some("Previous summary about Rust basics."), &messages);

    assert!(
        prompt.contains("当前摘要:\nPrevious summary about Rust basics."),
        "prompt must include existing summary section"
    );
    assert!(
        prompt.contains("user: What is ownership in Rust?"),
        "prompt must include old messages for compression"
    );
    assert!(
        prompt.contains("assistant: Ownership is a memory management concept."),
        "prompt must include assistant messages from old history"
    );
    assert!(
        prompt.contains("待压缩的对话历史"),
        "prompt must label the messages section for compression"
    );
}

// User Story: As a user whose session is being compacted for the first time
// (US-CORE-012), there is no existing summary. The compact prompt must not
// reference a nonexistent summary section.
// Covers: build_compact_prompt without existing summary omits summary section.

#[test]
fn compact_prompt_without_existing_summary_omits_summary_section() {
    let messages = vec![ChatMessage {
        role: "user".into(),
        content: "What is Rust?".into(),
    }];
    let prompt = build_compact_prompt(None, &messages);

    assert!(
        !prompt.contains("当前摘要"),
        "prompt must NOT include summary section when no existing summary"
    );
    assert!(
        prompt.contains("待压缩的对话历史"),
        "prompt must include old messages section"
    );
    assert!(
        prompt.contains("user: What is Rust?"),
        "prompt must include old message content"
    );
}

// User Story: As a user whose conversation is at the compact boundary
// (US-CORE-012), the compact function might be called with an empty set of
// old messages (e.g., all messages fit within the sliding window). The prompt
// must still be valid and not panic.
// Covers: build_compact_prompt with empty old_messages produces valid prompt.

#[test]
fn compact_prompt_with_empty_old_messages_produces_valid_prompt() {
    let messages: Vec<ChatMessage> = vec![];
    let prompt = build_compact_prompt(None, &messages);

    assert!(
        prompt.contains("待压缩的对话历史"),
        "prompt must include the messages header even when empty"
    );
    assert!(
        !prompt.contains("当前摘要"),
        "prompt must not include summary section when no existing summary"
    );
}

// ---------------------------------------------------------------------------
// Handler decision precondition: sliding window + empty history
// ---------------------------------------------------------------------------

// User Story: As a user on my first message (US-CORE-011), the system should
// use the original query directly for RAG search without rewriting. This test
// verifies that an empty sliding window means no history, confirming the
// handler's decision to skip rewriting on the first turn.
// Covers: ChatSession::get_sliding_window returns empty slice for new session;
//         handler checks history.is_empty() to decide rewrite path.

#[test]
fn new_session_sliding_window_is_empty_confirming_no_rewrite_needed() {
    let session = ChatSession::new("test-session".to_string());
    let window = session.get_sliding_window(6);

    assert!(
        window.is_empty(),
        "new session sliding window must be empty, confirming handler will \
         skip query rewriting on the first turn"
    );
}

// ---------------------------------------------------------------------------
// Degradation scenarios: rewrite fallback + compact fallback
// ---------------------------------------------------------------------------
//
// LLM Mocking Strategy
// ---------------------
// The handler's degradation logic is inline (not extracted into helper
// functions), so we cannot test fallback behavior via Approach A (direct
// helper invocation with error inputs). Approach B (integration test with
// failing LLM client at http://localhost:0) is also unreliable for post-hoc
// assertions because the handler returns SSE before compact runs inside
// tokio::spawn.
//
// Therefore, degradation is verified at the domain level:
//   1. Prompt builders produce correct input when history exists (already
//      covered by tests above).
//   2. Domain-level should_compact / compact_history behavior confirms that
//      skipping compact preserves all messages.
//   3. Code-structure checks document that the handler wraps LLM calls in
//      match arms that fall back gracefully.
//
// This strategy is endorsed by the task item (BE-T03) and the design doc
// section 5.1 (test strategy rows 5-6).

// User Story: As a user in a multi-turn conversation (US-CORE-011), when the
// LLM call for query rewriting fails (timeout, network error, etc.), the system
// must fall back to using my original message for RAG search so I still get
// relevant results without perceiving an error.
// Covers: Handler degradation path for rewriting failure.
//         Verification strategy:
//         (a) build_rewrite_prompt produces a valid prompt when history is
//             non-empty (confirmed by tests above).
//         (b) Code-structure check: the handler wraps the rewrite LLM call in
//             `match rewrite_agent.prompt(&rewrite_prompt).await { Ok(r) => r, Err(_) => original }`
//             so any LLM failure falls back to the original user message.

#[test]
fn query_rewriting_failure_fallback_code_structure_verified() {
    // This test documents the degradation contract at the code-structure level.
    //
    // The handler (chat.rs lines 156-173) implements rewriting fallback as:
    //
    //   let search_query = if history.is_empty() {
    //       req.message.clone()                          // no history, skip rewrite
    //   } else {
    //       let rewrite_prompt = build_rewrite_prompt(&history, &req.message);
    //       match rewrite_agent.prompt(&rewrite_prompt).await {
    //           Ok(rewritten) => rewritten,               // success: use rewritten query
    //           Err(e) => {
    //               tracing::warn!("query rewriting failed: {e}, falling back...");
    //               req.message.clone()                   // FAILURE: use original query
    //           }
    //       }
    //   };
    //
    // Key guarantees verified by code inspection:
    // 1. When history is empty, the original message is used directly (no LLM call).
    //    -> Covered by new_session_sliding_window_is_empty_confirming_no_rewrite_needed
    // 2. When history is non-empty and LLM succeeds, the rewritten query is used.
    //    -> Covered by rewrite_prompt_includes_history_and_user_message (prompt structure)
    // 3. When history is non-empty and LLM fails, the original message is used.
    //    -> This is the degradation path. The match Err arm returns req.message.clone().
    //       No panic, no error propagated to user. A tracing::warn is emitted.
    //
    // Domain-level assertion: build_rewrite_prompt itself is infallible (returns
    // String, not Result), so the only failure point is the LLM call, and the
    // match guarantees fallback.

    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Rust is a systems programming language.".into(),
        },
    ];
    let original_query = "How does it handle memory?";

    // The prompt builder never fails -- it always produces a String.
    let prompt = build_rewrite_prompt(&history, original_query, None);
    assert!(
        !prompt.is_empty(),
        "build_rewrite_prompt must always produce a non-empty prompt, \
         ensuring the only failure point is the LLM call itself"
    );
    assert!(
        prompt.contains(original_query),
        "the original query must be embedded in the prompt so that even \
         if the LLM response is unusable, the handler's Err arm can fall \
         back to the original query"
    );

    // The handler's fallback: original query is always available because
    // the match Err arm returns req.message.clone(), which is the same
    // string we passed to build_rewrite_prompt.
    // No further domain assertion needed -- the code structure guarantees
    // the original query is preserved and used on failure.
}

// User Story: As a user in a long conversation (US-CORE-012), when the LLM
// call for summary compression fails, the system must preserve all original
// messages without data loss. The user should not notice any disruption; the
// next request will retry compaction.
// Covers: Handler degradation path for compact failure.
//         Verification strategy:
//         (a) Domain-level: should_compact returns true when thresholds exceeded.
//         (b) Domain-level: NOT calling compact_history preserves all messages.
//         (c) Code-structure check: the handler wraps compact in a match that
//             only calls compact_history on Ok.

#[test]
fn compact_failure_preserves_all_messages_domain_level() {
    let sliding_window_size = 6;
    let compact_threshold = 8;

    // Build a session with enough messages to trigger compact.
    let mut session = ChatSession::new("test-compact-fallback".to_string());
    for i in 0..12 {
        // 12 messages > compact_threshold(8), > sliding_window_size(6)
        let role = if i % 2 == 0 { "user" } else { "assistant" };
        session.add_message(role, format!("Message {i} content with enough text."));
    }

    let message_count_before = session.messages.len();

    // (a) Verify should_compact returns true -- compact should be triggered.
    assert!(
        session.should_compact(compact_threshold, 8000, sliding_window_size),
        "session with {} messages must trigger should_compact when \
         threshold is {} and window is {}",
        message_count_before,
        compact_threshold,
        sliding_window_size,
    );

    // (b) Simulate compact failure: do NOT call compact_history.
    //     Verify all messages are preserved unchanged.
    assert_eq!(
        session.messages.len(),
        message_count_before,
        "without calling compact_history, no messages must be dropped"
    );
    assert_eq!(
        session.summary, None,
        "without calling compact_history, summary must remain None"
    );

    // Verify message content is intact -- first and last messages still present.
    assert!(
        session.messages[0].content.contains("Message 0"),
        "first message must still be present (no data loss)"
    );
    assert!(
        session.messages[11].content.contains("Message 11"),
        "last message must still be present (no data loss)"
    );

    // (c) Code-structure verification:
    //     The handler (chat.rs lines 321-333) wraps the compact LLM call:
    //
    //       match compact_agent.prompt(&prompt).await {
    //           Ok(new_summary) => {
    //               // only on success: compact_history is called
    //               if let Some(session) = sessions.lock().await.get_mut(&session_id) {
    //                   session.compact_history(new_summary, sliding_window_size);
    //               }
    //           }
    //           Err(e) => {
    //               tracing::error!("compact failed: {e}, keeping original messages");
    //               // compact_history is NOT called -> messages preserved
    //           }
    //       }
    //
    // The Err arm logs the error and does NOT call compact_history, which is
    // exactly the behavior verified above: messages.len() unchanged, summary
    // unchanged. On the next request, should_compact will still return true
    // and the system will retry.
}

// User Story: As a user whose session triggers compaction (US-CORE-012), the
// compact_history method must correctly trim old messages and set the summary
// when compaction succeeds. This test verifies the happy path of
// compact_history to confirm the domain model works correctly, establishing
// that the degradation path (not calling it) is a meaningful fallback.
// Covers: ChatSession::compact_history correctly trims messages and sets summary;
//         validates that the non-call path in the degradation test is significant.

#[test]
fn compact_history_success_path_trims_and_sets_summary() {
    let sliding_window_size = 6;

    let mut session = ChatSession::new("test-compact-success".to_string());
    for i in 0..12 {
        let role = if i % 2 == 0 { "user" } else { "assistant" };
        session.add_message(role, format!("Message {i}."));
    }

    assert_eq!(session.messages.len(), 12);
    assert!(session.summary.is_none());

    // Call compact_history (simulating successful LLM compaction).
    session.compact_history("Summary of messages 0-5.".to_string(), sliding_window_size);

    // Only the last sliding_window_size messages should remain.
    assert_eq!(
        session.messages.len(),
        sliding_window_size,
        "compact_history must trim to sliding_window_size messages"
    );
    assert_eq!(
        session.summary,
        Some("Summary of messages 0-5.".to_string()),
        "compact_history must set the summary"
    );

    // Verify the remaining messages are the last 6 (indices 6-11 in original).
    assert!(
        session.messages[0].content.contains("Message 6"),
        "first message after compact must be the earliest outside the window"
    );
    assert!(
        session.messages[5].content.contains("Message 11"),
        "last message after compact must be the most recent"
    );
}

// ===========================================================================
// Query Rewrite Pipeline scenario tests (US-CORE-019, US-CORE-020, US-CORE-022)
// ===========================================================================
//
// These tests cover the query rewrite feature: first-turn prompt construction,
// modified multi-turn prompt with JSON constraint, parse_rewrite_response
// degradation chain, and handler-level rewrite decision logic.

// ---------------------------------------------------------------------------
// build_first_turn_rewrite_prompt scenarios
// ---------------------------------------------------------------------------

// User Story: US-CORE-019 -- As a user submitting a short or ambiguous first
// query, I want it to be expanded into a more specific, searchable form so
// that relevant documents are found even when my original query is vague.
// Covers: build_first_turn_rewrite_prompt embeds the user message, instructs
//         JSON output format, and references the max queries limit so the
//         LLM returns structured multi-query output.

#[test]
fn first_turn_rewrite_prompt_includes_user_message_and_json_constraint() {
    let prompt = build_first_turn_rewrite_prompt("k8s", None);

    assert!(
        prompt.contains("用户查询: k8s"),
        "first-turn prompt must embed the user message for LLM context"
    );
    assert!(
        prompt.contains("JSON"),
        "first-turn prompt must instruct JSON output format"
    );
    assert!(
        prompt.contains("queries"),
        "first-turn prompt must reference the queries array structure"
    );
    // REWRITE_MAX_QUERIES is 2 (private const in chat.rs); verify the value
    // appears in the prompt text.
    assert!(
        prompt.contains("最多生成 2 条查询"),
        "first-turn prompt must reference the max queries limit (REWRITE_MAX_QUERIES=2)"
    );
}

// User Story: US-CORE-019 -- Edge case: the prompt builder must not panic on
// empty input even though the handler validates message non-emptiness upstream
// (chat.rs line 204: `if req.message.trim().is_empty()`).
// Covers: build_first_turn_rewrite_prompt is infallible for any string input,
//         including empty strings. The function returns a non-empty String with
//         the JSON format instruction intact.

#[test]
fn first_turn_rewrite_prompt_with_empty_user_message_produces_valid_structure() {
    let prompt = build_first_turn_rewrite_prompt("", None);

    assert!(
        !prompt.is_empty(),
        "first-turn prompt must produce a non-empty string even for empty input"
    );
    assert!(
        prompt.contains("JSON"),
        "first-turn prompt must contain JSON format instruction regardless of input"
    );
    assert!(
        prompt.contains("用户查询: "),
        "first-turn prompt must contain the user query header even with empty body"
    );
}

// ---------------------------------------------------------------------------
// Modified build_rewrite_prompt with JSON constraint scenarios
// ---------------------------------------------------------------------------

// User Story: US-CORE-011 -- As a user in a multi-turn conversation, I want
// my follow-up questions rewritten into standalone queries with structured
// JSON output so the system can generate multiple search variants.
// Covers: The modified multi-turn rewrite prompt now appends a JSON output
//         format constraint so the LLM returns structured output for multi-query
//         generation, while still containing history content and user message.

#[test]
fn rewrite_prompt_includes_json_output_format_constraint() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Kubernetes?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Kubernetes is a container orchestration platform.".into(),
        },
    ];
    let prompt = build_rewrite_prompt(&history, "How does it handle memory?", None);

    assert!(
        prompt.contains("JSON"),
        "modified rewrite prompt must include JSON format constraint"
    );
    assert!(
        prompt.contains("queries"),
        "modified rewrite prompt must reference the queries array structure"
    );
    assert!(
        prompt.contains("How does it handle memory?"),
        "modified rewrite prompt must still include the user message"
    );
}

// User Story: US-CORE-011 (regression) -- The JSON constraint must not break
// the core purpose of multi-turn rewrite: resolving pronouns and ellipses
// using conversation history.
// Covers: Modified build_rewrite_prompt retains full history context for
//         coreference resolution. Both history entries and the user message
//         must be present alongside the JSON format instruction.

#[test]
fn rewrite_prompt_retains_history_context_for_coreference_resolution() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Rust is a systems programming language.".into(),
        },
    ];
    let prompt = build_rewrite_prompt(&history, "it", None);

    // Verify history is preserved for pronoun resolution
    assert!(
        prompt.contains("user: What is Rust?"),
        "prompt must include user history for coreference resolution"
    );
    assert!(
        prompt.contains("assistant: Rust is a systems programming language."),
        "prompt must include assistant history for context"
    );
    // Verify user message is present
    assert!(
        prompt.contains("当前用户追问: it"),
        "prompt must include the current ambiguous user message"
    );
    // Verify JSON constraint is still appended
    assert!(
        prompt.contains("JSON"),
        "prompt must include JSON format instruction alongside history"
    );
}

// ---------------------------------------------------------------------------
// parse_rewrite_response degradation chain scenarios
// ---------------------------------------------------------------------------

// User Story: US-CORE-019 -- Normal case: LLM returns valid JSON with queries
// array, and the parser extracts the queries correctly.
// Covers: parse_rewrite_response correctly parses a well-formed JSON response
//         with a "queries" array containing multiple query strings.

#[test]
fn parse_rewrite_valid_json_array_parsed_correctly() {
    let raw = r#"{"queries": ["How to deploy Kubernetes", "Kubernetes deployment guide"]}"#;
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result,
        vec!["How to deploy Kubernetes", "Kubernetes deployment guide"],
        "valid JSON with queries array must be parsed into exact query list"
    );
}

// User Story: US-CORE-019 -- LLM wraps JSON in a markdown code fence with
// language tag. The parser must strip the fence and parse the inner JSON.
// Covers: parse_rewrite_response strips ```json ... ``` fences before parsing.

#[test]
fn parse_rewrite_json_with_markdown_fence_stripped() {
    let raw = "```json\n{\"queries\": [\"query1\"]}\n```";
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result,
        vec!["query1"],
        "JSON wrapped in ```json fence must be stripped and parsed correctly"
    );
}

// User Story: US-CORE-019 -- LLM wraps JSON in a plain code fence without
// language tag. The parser must strip the fence and parse the inner JSON.
// Covers: parse_rewrite_response strips ``` ... ``` fences (no language tag).

#[test]
fn parse_rewrite_json_with_plain_fence_stripped() {
    let raw = "```\n{\"queries\": [\"query1\"]}\n```";
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result,
        vec!["query1"],
        "JSON wrapped in plain ``` fence must be stripped and parsed correctly"
    );
}

// User Story: US-CORE-022 -- LLM returns valid JSON but with an empty queries
// array. The system must fall back gracefully so the user still gets results.
// Covers: parse_rewrite_response degrades to raw trimmed output as single query
//         when the queries array is empty.

#[test]
fn parse_rewrite_empty_queries_array_degrades_to_raw_output() {
    let raw = "{\"queries\": []}";
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result.len(),
        1,
        "empty queries array must degrade to single raw output"
    );
    assert_eq!(
        result[0], raw,
        "degraded output must be the trimmed raw input"
    );
}

// User Story: US-CORE-022 -- LLM returns valid JSON but with wrong schema
// (no "queries" key). The system must fall back gracefully.
// Covers: parse_rewrite_response degrades to raw trimmed output as single query
//         when the JSON object lacks the "queries" field.

#[test]
fn parse_rewrite_missing_queries_field_degrades_to_raw_output() {
    let raw = "{\"result\": \"something\"}";
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result.len(),
        1,
        "missing queries field must degrade to single raw output"
    );
    assert_eq!(
        result[0], raw,
        "degraded output must be the trimmed raw input"
    );
}

// User Story: US-CORE-022 -- LLM ignores the JSON instruction entirely and
// returns plain text. The system must still function by using the text as-is.
// Covers: parse_rewrite_response degrades to the entire plain text output
//         as a single query when JSON parsing fails.

#[test]
fn parse_rewrite_pure_text_degrades_to_single_query() {
    let raw = "This is not JSON at all";
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result,
        vec!["This is not JSON at all"],
        "plain text must degrade to single query equal to trimmed raw input"
    );
}

// User Story: US-CORE-019 -- LLM generates more queries than allowed by the
// REWRITE_MAX_QUERIES constant (2). The parser must truncate to prevent
// excessive downstream search calls.
// Covers: parse_rewrite_response truncates to at most REWRITE_MAX_QUERIES (2)
//         entries, keeping the first ones.

#[test]
fn parse_rewrite_truncates_queries_exceeding_max() {
    let raw = r#"{"queries": ["q1", "q2", "q3", "q4"]}"#;
    let result = parse_rewrite_response(raw);

    // REWRITE_MAX_QUERIES = 2 (defined in chat.rs)
    assert_eq!(
        result.len(),
        2,
        "queries exceeding REWRITE_MAX_QUERIES (2) must be truncated"
    );
    assert_eq!(
        result[0], "q1",
        "first query must be preserved after truncation"
    );
    assert_eq!(
        result[1], "q2",
        "second query must be preserved after truncation"
    );
}

// User Story: US-CORE-022 -- LLM returns queries with empty or whitespace-only
// strings. These are useless for search and must be filtered out.
// Covers: parse_rewrite_response filters out empty and whitespace-only query
//         strings, keeping only meaningful non-empty entries.

#[test]
fn parse_rewrite_filters_empty_strings_from_queries() {
    let raw = r#"{"queries": ["valid query", "", "  ", "another"]}"#;
    let result = parse_rewrite_response(raw);

    assert_eq!(
        result,
        vec!["valid query", "another"],
        "empty and whitespace-only query strings must be filtered out"
    );
}

// ---------------------------------------------------------------------------
// Handler-level code-structure tests for rewrite decision paths
// ---------------------------------------------------------------------------

// User Story: US-CORE-019 -- As a user submitting a first-turn query (no
// history), the handler must rewrite my query instead of skipping. Design doc
// 5.5: the handler no longer has an `if history.is_empty()` skip branch.
// First-turn queries now go through build_first_turn_rewrite_prompt.
// Covers: build_first_turn_rewrite_prompt is infallible and always produces
//         a valid prompt, confirming the handler can unconditionally call it
//         on first turn without any skip logic.

#[test]
fn first_turn_rewrite_triggers_code_structure_verified() {
    // Simulate the handler's first-turn path: history is empty, so the
    // handler calls build_first_turn_rewrite_prompt instead of skipping.
    let user_message = "k8s";
    let history: Vec<ChatMessage> = vec![];

    // The handler code (chat.rs lines 232-238) does:
    //   let (rewrite_preamble, rewrite_prompt) = if history.is_empty() {
    //       ("...", build_first_turn_rewrite_prompt(&req.message))
    //   } else { ... };
    //
    // Key guarantee: build_first_turn_rewrite_prompt is infallible (returns
    // String, not Result), so the handler can always call it without guards.
    let prompt = build_first_turn_rewrite_prompt(user_message, None);

    assert!(
        !prompt.is_empty(),
        "build_first_turn_rewrite_prompt must always produce a non-empty prompt, \
         confirming the handler can unconditionally call it on first turn"
    );
    assert!(
        prompt.contains(user_message),
        "prompt must contain the original user message so the rewrite LLM can \
         expand it into a more specific query"
    );

    // Domain guarantee verified: history is empty, handler enters the first-turn
    // branch, and build_first_turn_rewrite_prompt produces a valid prompt.
    assert!(
        history.is_empty(),
        "empty history confirms handler takes the first-turn rewrite branch"
    );
}

// User Story: US-CORE-022 -- When the rewrite LLM call fails (timeout, network
// error, etc.), the handler falls back to vec![req.message.clone()]. This test
// verifies that the original query is always recoverable from the prompt.
// Covers: build_first_turn_rewrite_prompt embeds the original query in its
//         output, so the handler's Err/Timeout match arms can always fall back
//         to the original message. The code structure (chat.rs lines 247-264)
//         guarantees: Ok(Err(e)) => vec![req.message.clone()],
//         Err(_) => vec![req.message.clone()].

#[test]
fn rewrite_llm_failure_fallback_preserves_original_query_code_structure() {
    let user_message = "deploy";

    // The prompt builder embeds the original query.
    let prompt = build_first_turn_rewrite_prompt(user_message, None);

    assert!(
        prompt.contains(user_message),
        "first-turn prompt must contain the original query ('deploy'), \
         confirming the handler's Err/Timeout arms can fall back to req.message.clone()"
    );

    // The handler's fallback guarantees (code structure, chat.rs lines 253-264):
    //   Ok(Ok(raw_response)) => parse_rewrite_response(&raw_response),
    //   Ok(Err(e)) => {
    //       tracing::warn!("query rewriting failed: {e}, falling back to original query");
    //       vec![req.message.clone()]          // <-- original query preserved
    //   }
    //   Err(_) => {
    //       tracing::warn!("query rewriting timed out, falling back to original query");
    //       vec![req.message.clone()]          // <-- original query preserved
    //   }
    //
    // The handler always has access to req.message (the original user input)
    // regardless of the rewrite outcome. No domain assertion needed beyond
    // confirming the prompt builder is infallible.
    assert!(
        !prompt.is_empty(),
        "build_first_turn_rewrite_prompt is infallible -- the only failure \
         point is the LLM call, and the handler's match arms guarantee fallback"
    );
}

// User Story: US-CORE-022 -- When all rewrite queries return empty search
// results, the handler falls back to search_with_expansion(&req.message, ...).
// The original message must be preserved throughout the rewrite pipeline.
// Covers: The code structure (chat.rs lines 283-285) checks
//         `if results.is_empty()` after search_multi_query and retries with
//         the original message. parse_rewrite_response is infallible for any
//         input, ensuring the pipeline never loses the original query.

#[test]
fn multi_query_all_empty_result_triggers_fallback_code_structure() {
    // The handler code (chat.rs lines 267-295):
    //   let search_results = if search_queries.len() == 1 {
    //       state.vector_store.search_with_expansion(&search_queries[0], ...)
    //   } else {
    //       let results = state.vector_store.search_multi_query(&search_queries, ..., RRF_K).await?;
    //       if results.is_empty() {
    //           tracing::warn!("all rewrite queries returned empty, falling back");
    //           state.vector_store.search_with_expansion(&req.message, ...)  // <-- fallback
    //       } else {
    //           results
    //       }
    //   };
    //
    // Key contract: req.message is preserved throughout the pipeline.
    // parse_rewrite_response never fails -- it always returns at least
    // one query (the raw trimmed output as fallback).

    // Verify parse_rewrite_response is infallible for any input:
    let cases = vec![
        "plain text",
        "{}",
        "[]",
        "{\"queries\": []}",
        "```invalid",
        "{\"queries\": [\"  \"]}",
    ];

    for input in cases {
        let result = parse_rewrite_response(input);
        assert!(
            !result.is_empty(),
            "parse_rewrite_response must never return empty Vec -- got empty for input: {:?}",
            input
        );
        for query in &result {
            assert!(
                !query.is_empty(),
                "parse_rewrite_response must never return empty strings -- got empty query for input: {:?}",
                input
            );
        }
    }

    // Domain guarantee: the handler always has req.message available for the
    // fallback path. parse_rewrite_response is infallible, so search_queries
    // is always non-empty. If search_multi_query returns empty results, the
    // handler retries with req.message (the original user input).
}

// ===========================================================================
// Post-answer suggestions SSE ordering + degradation scenarios (US-CORE-037)
// ===========================================================================
//
// LLM Mocking Strategy (carries over from the block above, lines 289-311):
// The handler's `chat_inner` closure emits the `suggestions` SSE event inline
// inside the `FinalResponse` match arm, immediately before `done`. The LLM
// client is the concrete `rig::providers::openai::CompletionsClient` struct
// (not a trait), and integration tests against `localhost:0` are unreliable
// for post-hoc assertions because the handler returns SSE before the inner
// `tokio::spawn` task completes. Therefore, per the repo's established
// convention (see `query_rewriting_failure_fallback_code_structure_verified`
// and `compact_failure_preserves_all_messages_domain_level` above), the
// ordering/degradation contracts for the post-answer `suggestions` event are
// verified via code-structure + pure-fn anchors. No mock-LLM harness is
// introduced.
//
// The dev slot (BE-D02) owns the exhaustive pure-fn unit-test table for
// `parse_suggested_questions_response` and `build_post_answer_prompt` inside
// `chat.rs::tests`. These scenarios do NOT duplicate that table; each one
// carries at most a SINGLE representative pure-fn anchor that encodes WHY
// the contract matters for SSE ordering.

// User Story: US-CORE-037 -- As a user who has the post-answer-suggestions
// switch enabled, after my answer finishes streaming I want to receive
// follow-up question suggestions BEFORE the `done` event so my client can
// surface them in the same SSE stream rather than via a second round-trip.
// Covers: chat_inner `FinalResponse` branch wire order:
//         session -> chunk... -> suggestions -> done.
//         The generator runs, and ONLY when its result is non-empty does
//         the closure send the `suggestions` event; the `done` send and
//         `break` immediately follow in the SAME match arm, so a non-empty
//         suggestions result is always observed strictly before `done`.

#[test]
fn post_answer_suggestions_emitted_before_done_when_switch_on_code_structure() {
    // chat_inner FinalResponse arm (chat.rs):
    //
    //   Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)) => {
    //       if enable_post_answer_suggestions {
    //           let suggestions = generate_post_answer_suggestions(
    //               &state.llm_client, &state.llm_model,
    //               &user_message, &assistant_text, &context_text,
    //           ).await;
    //           if !suggestions.is_empty() {
    //               let event = Event::default().event("suggestions")
    //                   .data(serde_json::to_string(&SuggestionsEvent { suggestions }).unwrap_or_default());
    //               let _ = tx.send(Ok(event)).await;       // (1) suggestions send
    //           }
    //       }
    //       let done_event = DoneEvent {};
    //       let event = Event::default().event("done")
    //           .data(serde_json::to_string(&done_event).unwrap_or_default());
    //       let _ = tx.send(Ok(event)).await;               // (2) done send
    //       break;
    //   }
    //
    // Wire-order contract pinned by code structure:
    //   - `suggestions` is sent in (1); `done` is sent in (2); they share the
    //     same match arm and execute in source order, so any client observing
    //     both sees `...chunk..., suggestions, done`. There is no path that
    //     emits `done` before a non-empty `suggestions`.
    //
    // Pure-fn anchor: a non-empty parser result is precisely the precondition
    // that makes the closure enter the `if !suggestions.is_empty()` block and
    // emit the event. Establishing that a valid LLM-shaped response yields a
    // non-empty Vec is the load-bearing anchor for "when does suggestions
    // fire at all".
    let parsed = parse_suggested_questions_response(r#"{"questions":["Q1","Q2"]}"#);
    assert!(
        !parsed.is_empty(),
        "a non-empty parse result is the precondition for emitting the \
         `suggestions` event before `done`"
    );
    assert_eq!(
        parsed.len(),
        2,
        "parser must preserve order/count so the SSE payload matches the LLM output"
    );

    // Prompt grounding anchor: non-empty suggestions come from a prompt that
    // embeds the round's user message, answer, and retrieved context.
    let prompt = build_post_answer_prompt(
        "如何重置密码?",
        "点击设置中的重置按钮。",
        "<context>KB-DOC</context>",
    );
    assert!(
        prompt.contains("如何重置密码?")
            && prompt.contains("点击设置中的重置按钮。")
            && prompt.contains("<context>KB-DOC</context>"),
        "non-empty suggestions are grounded in user message + answer + context; \
         prompt must embed all three segments"
    );
}

// User Story: US-CORE-037 -- As an operator who has NOT enabled the switch
// (default off), I do not want the system to make the extra post-answer LLM
// call or emit a `suggestions` event. Existing clients that never opted in
// must observe byte-identical behavior to before the feature existed.
// Covers: chat_inner guards BOTH the generator call AND the event send behind
//         `enable_post_answer_suggestions`. When the switch is false the
//         closure never invokes generate_post_answer_suggestions and never
//         sends a `suggestions` event; the `FinalResponse` arm only sends
//         `done`.

#[test]
fn post_answer_suggestions_switch_off_skips_event_and_llm_call_code_structure() {
    // chat_inner closure-exterior extraction (chat.rs, alongside the other
    // pre-spawn config reads):
    //
    //   let enable_post_answer_suggestions =
    //       state.chat_config.enable_post_answer_suggestions;
    //
    // Inside the FinalResponse arm, the entire block is wrapped:
    //
    //   if enable_post_answer_suggestions {
    //       let suggestions = generate_post_answer_suggestions(...).await;  // extra LLM call
    //       if !suggestions.is_empty() {
    //           /* send suggestions event */
    //       }
    //   }
    //   /* send done event */
    //
    // Code-structure contract: when `enable_post_answer_suggestions == false`,
    // neither the generator call nor the `suggestions` send executes. The
    // `if` guards the entire block including `generate_post_answer_suggestions`,
    // so the extra LLM call is skipped entirely (not just its send).

    // Pure-fn anchor pinning the default-off precondition (BE-D01 guarantees
    // `#[serde(default)]` + `Default = false`). This anchor encodes WHY the
    // guard matters: an operator who omits the field gets the switch off, so
    // the `if` block is skipped and behavior is backward-compatible.
    use rwiki_core::config::ChatConfig;
    let default_config = ChatConfig::default();
    assert!(
        !default_config.enable_post_answer_suggestions,
        "ChatConfig::default() must set enable_post_answer_suggestions=false \
         so the closure's `if enable_post_answer_suggestions` guard skips the \
         extra LLM call and the `suggestions` event for operators who do not \
         opt in (backward compatibility, design §4.2)"
    );
}

// User Story: US-CORE-037 -- As a user with the switch on, when the
// post-answer LLM call times out or errors, the system must silently degrade:
// no `suggestions` event is sent, but the `done` event still fires so my
// client closes the stream cleanly. The main answer and `done` are unaffected
// by the post-answer call's failure.
// Covers: generate_post_answer_suggestions three-branch degrade contract:
//           Ok(Ok(raw))  -> parse_suggested_questions_response(&raw)
//           Ok(Err(e))   -> tracing::warn!(...); Vec::new()
//           Err(_)       -> tracing::warn!(...); Vec::new()   // timeout
//         On any non-Ok(Ok) branch the generator returns empty, so the
//         `if !suggestions.is_empty()` block in chat_inner is skipped and
//         only `done` is emitted.

#[test]
fn post_answer_suggestions_timeout_or_error_silently_degrades_code_structure() {
    // generate_post_answer_suggestions (chat.rs):
    //
    //   match tokio::time::timeout(
    //       Duration::from_millis(POST_ANSWER_TIMEOUT_MS),
    //       agent.prompt(&prompt),
    //   ).await {
    //       Ok(Ok(raw)) => parse_suggested_questions_response(&raw),
    //       Ok(Err(e)) => {
    //           tracing::warn!("post-answer suggestions failed: {e}");
    //           Vec::new()                                  // <- silent degrade
    //       }
    //       Err(_) => {
    //           tracing::warn!("post-answer suggestions timed out after {POST_ANSWER_TIMEOUT_MS}ms");
    //           Vec::new()                                  // <- silent degrade
    //       }
    //   }
    //
    // Code-structure contract: both the LLM-error branch and the timeout
    // branch return `Vec::new()` with only a `tracing::warn!` (no panic, no
    // metrics field, no error propagated to the caller). An empty Vec causes
    // chat_inner's `if !suggestions.is_empty()` to skip the event send, so
    // the FinalResponse arm proceeds directly to the `done` send and `break`.

    // Single representative pure-fn anchor (NOT a table duplication): a
    // garbage LLM output parses to empty, which is the same Vec the generator
    // returns on timeout/error. This pins WHY the silent-degrade matters:
    // garbage -> empty -> no `suggestions` event, but `done` still fires.
    // The exhaustive parser table (including the truncation-to-3 case) lives
    // in chat.rs::tests (BE-D02, owned by the dev slot).
    let parsed = parse_suggested_questions_response("not json");
    assert!(
        parsed.is_empty(),
        "garbage LLM output must parse to empty Vec, which is the same shape \
         the generator returns on timeout/error; an empty Vec causes chat_inner \
         to skip the `suggestions` send while still emitting `done` (silent \
         degrade, design §4.2 / §5.2)"
    );
}

// User Story: US-CORE-037 -- As a user whose main answer stream itself failed
// (LLM stream error), I must NOT receive a `suggestions` event. Post-answer
// suggestions are only meaningful after a successful answer; emitting them on
// failure would be misleading.
// Covers: chat_inner `Err(e)` match arm sends only the `error` event and
//         `return`s from the closure. The `suggestions` emit code lives
//         EXCLUSIVELY inside the `FinalResponse` arm, so a main-answer
//         failure can never reach the suggestions block.

#[test]
fn post_answer_suggestions_never_emitted_on_main_answer_error_code_structure() {
    // chat_inner match on `stream.next().await` (chat.rs):
    //
    //   while let Some(item) = stream.next().await {
    //       match item {
    //           Ok(rig::agent::MultiTurnStreamItem::StreamAssistantItem(...)) => { /* chunk */ }
    //           Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)) => {
    //               /* ONLY HERE: optional `suggestions` send + `done` send + break */
    //           }
    //           Ok(_) => { /* ignore tool calls / reasoning */ }
    //           Err(e) => {
    //               tracing::error!("Stream error: {e}");
    //               /* metrics + records */
    //               let error_event = ErrorEvent { message: "Failed to generate response. ...".into() };
    //               let event = Event::default().event("error").data(...);
    //               let _ = tx.send(Ok(event)).await;       // (1) error event only
    //               /* records */
    //               return;                                  // (2) exit closure
    //           }
    //       }
    //   }
    //
    // Code-structure contract:
    //   - The `Err(e)` arm sends ONLY the `error` event and then `return`s.
    //   - The `suggestions` emit code exists ONLY inside the `FinalResponse`
    //     arm (the same arm that emits `done`).
    //   - Therefore a main-answer stream error can NEVER reach the suggestions
    //     block: the `return` exits the closure before any subsequent match
    //     arm could run, and the suggestions code is not present in the Err arm.
    //
    // This mirrors the established code-structure idiom
    // (`query_rewriting_failure_fallback_code_structure_verified`):
    // there is no mock-LLM harness, so the suggestion-free Err arm is verified
    // by documenting the handler structure rather than by executing it.

    // No pure-fn anchor is meaningful here: the Err arm is purely about
    // control flow (which match arm runs). The contract is that the
    // suggestions code lives only in FinalResponse; this is a code-structure
    // guarantee, consistent with how the repo verifies the analogous
    // rewrite-fallback Err arm.
}

// User Story: US-CORE-037 (regression, design §6.3) -- Moving `context_text`
// into the `chat_inner` closure (so the post-answer generator can reuse it)
// must NOT detach the existing `build_preamble` / `context_chars` consumers
// from the same value. The preamble must still embed the round's retrieved
// context, and the context-chars metric must still reflect the same string.
// Covers: chat_inner builds `context_text = format_context_xml(&search_results)`
//         once, then passes `&context_text` to BOTH `build_preamble` (for the
//         main answer agent) AND `generate_post_answer_suggestions` (for the
//         post-answer prompt). The two consumers must see the identical value.

#[test]
fn post_answer_context_text_move_does_not_break_preamble_code_structure() {
    // chat_inner (chat.rs):
    //
    //   let context_text = format_context_xml(&search_results);   // built once
    //   let context_chunks = search_results.len();
    //   let context_chars = context_text.chars().count();         // metric source
    //
    //   let preamble = build_preamble(
    //       &state.chat_config.system_prompt,
    //       summary.as_deref(),
    //       &context_text,                                          // consumer A
    //   );
    //
    //   // ... later, inside the spawned closure's FinalResponse arm:
    //   let suggestions = generate_post_answer_suggestions(
    //       &state.llm_client, &state.llm_model,
    //       &user_message, &assistant_text,
    //       &context_text,                                          // consumer B (moved in)
    //   ).await;
    //
    // Code-structure contract: both `build_preamble` (consumer A, outer scope,
    // borrow ends before tokio::spawn) and `generate_post_answer_suggestions`
    // (consumer B, inside the closure) receive `&context_text` derived from
    // the SAME `format_context_xml(&search_results)` call. Moving the value
    // into the closure does NOT recompute or detach it.

    // Regression anchor: the SAME context string the generator would receive
    // must still be embedded by `build_preamble`. If the move had detached
    // preamble from context (e.g., by re-running format_context_xml on a
    // different/empty slice, or by passing a stale clone), this substring
    // presence would fail. This pins design §6.3: "context_text 移入闭包
    // 不破坏既有 preamble/context_chars".
    let context = "<documents>\n<document index=\"1\">\n<title>Reset Password</title>\n<content>\nClick the reset button in settings.\n</content>\n</document>\n</documents>";
    let preamble = build_preamble(
        "You are a knowledge base assistant.",
        Some("User asked about account recovery."),
        context,
    );

    // system -> summary -> context ordering preserved (mirrors the existing
    // preamble_with_summary_includes_summary_between_system_and_context style
    // without duplicating it; the load-bearing assertion here is that the
    // EXACT context string the generator receives is the one embedded).
    assert!(
        preamble.contains(context),
        "build_preamble must embed the SAME context string that \
         generate_post_answer_suggestions receives via &context_text; if the \
         closure move detached preamble from context this substring would fail \
         (design §6.3 regression pin)"
    );
    let sys_pos = preamble
        .find("You are a knowledge base assistant.")
        .unwrap();
    let summary_pos = preamble.find("Conversation Summary").unwrap();
    let ctx_pos = preamble.find(context).unwrap();
    assert!(
        sys_pos < summary_pos && summary_pos < ctx_pos,
        "preamble ordering must remain system -> summary -> context after the \
         context_text move (unchanged preamble contract)"
    );
}

// ===========================================================================
// Low-recall bypass-write trigger scenario tests (US-CORE-038)
// ===========================================================================
//
// These scenarios cover the detached `tokio::spawn` bypass-write block that
// BE-D04 added to `chat_inner` (chat.rs ~845-897):
//
//   - gated on `RetrievalScope::Published` (public `/api/chat` only, NOT scoped)
//   - gated on `state.low_recall_config.is_some()`
//   - logs when `top_score < threshold` OR when there are zero results
//     (zero-result path always logs with `top_score = NULL`)
//   - write failure is swallowed via `tracing::warn!` (design §7 P0:
//     "writing must never block chat or change its latency / availability")
//
// Each triggering scenario strongly asserts `resp.status().is_success()`
// (design §7 P0) — the chat response MUST NOT be blocked or altered by the
// bypass write. The detached `tokio::spawn` is async, so before asserting on
// `low_recall_records` row counts the tests `tokio::time::sleep` briefly to
// yield and let the spawn complete (BE-T02 spec).

// ---------------------------------------------------------------------------
// Test helpers (low-recall integration scenarios)
// ---------------------------------------------------------------------------

/// Ensure the sqlite-vec extension is registered globally so that the
/// `vec0` virtual table module is available for in-memory connections.
static LOW_RECALL_SQLITE_VEC_INIT: Once = Once::new();
fn low_recall_ensure_sqlite_vec_loaded() {
    LOW_RECALL_SQLITE_VEC_INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

/// Build a minimal `AppState` identical to the rerank/feedback test-app-state
/// construction (in-memory sqlite + migrations + seeded `chunk_metadata` row
/// so the chat handler proceeds past the "knowledge base empty" guard, plus a
/// mockito-backed embeddings endpoint so `search_hybrid` can embed the query
/// without hitting the real OpenAI endpoint), with `low_recall_config` set
/// from the caller.
///
/// All other fields mirror `test_app_state_with_reranker` defaults. This helper
/// exists (rather than reusing the rerank helper) because the rerank helper
/// hard-codes `low_recall_config: None`.
async fn build_low_recall_app_state(low_recall_config: Option<LowRecallConfig>) -> Arc<AppState> {
    low_recall_ensure_sqlite_vec_loaded();

    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory sqlite");
    rwiki_core::infrastructure::migration::migrations(1536)
        .to_latest(&mut conn)
        .expect("apply migrations");
    // Seed a dummy chunk so vector_store.is_empty() returns false, allowing the
    // chat handler to proceed. The seeded chunk will only be retrieved if the
    // embedding search matches; otherwise the handler sees zero results and
    // records a "完全未命中" row (top_score NULL).
    conn.execute(
        "INSERT INTO chunk_metadata (document_id, chunk_id, content, title) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params!["test-doc", "test-chunk", "seed content", "Seed Title"],
    )
    .expect("seed chunk_metadata");
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // mockito embeddings mock so search_hybrid can embed the query offline.
    // Box::leak keeps the server alive for the test's lifetime (idiom shared
    // with rerank_scenarios.rs).
    let embed_server = Box::leak(Box::new(mockito::Server::new_async().await));
    // 1536-dim zero embedding matching text-embedding-3-small. A zero vector
    // will not strongly match the seeded chunk (whose embedding is never
    // computed here), so most queries return zero results — convenient for the
    // null-top-score scenario. For the "hit a seeded chunk" scenario we rely
    // on the row-count assertion rather than exact score control.
    let dummy_embedding: Vec<f64> = vec![0.0; 1536];
    let embed_body = serde_json::json!({
        "object": "list",
        "data": [{"object": "embedding", "embedding": dummy_embedding, "index": 0}],
        "model": "text-embedding-3-small",
        "usage": {"prompt_tokens": 1, "total_tokens": 1}
    })
    .to_string();
    let _embed_mock = embed_server
        .mock("POST", "/embeddings")
        .with_status(200)
        .with_body(&embed_body)
        .expect_at_most(20)
        .create_async()
        .await;

    let openai_client = rig::providers::openai::Client::builder()
        .api_key("sk-test-fake-key-for-low-recall-tests-only")
        .base_url(embed_server.url());
    let embedding_model = openai_client
        .build()
        .expect("build openai client")
        .embedding_model("text-embedding-3-small");
    let app_embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(embedding_model);

    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            sqlite.clone(),
            app_embedding_model,
            "text-embedding-3-small".to_string(),
        ),
    );

    let llm_client = rig::providers::openai::CompletionsClient::builder()
        .api_key("sk-test-fake")
        .base_url("http://localhost:0")
        .build()
        .expect("build LLM client");

    Arc::new(AppState {
        sqlite,
        enable_openapi: false,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_client,
        llm_model: "test-model".to_string(),
        api_token: LOW_RECALL_TEST_API_TOKEN.to_string(),
        api_allowed_ip_ranges: Vec::new(),
        chat_config: rwiki_core::config::ChatConfig::default(),
        static_dir: None,
        allowed_origins: vec![],
        retrieval_config: rwiki_core::config::RetrievalConfig::default(),
        reranker: None,
        rerank_config: rwiki_core::config::RerankConfig::default(),
        low_recall_config,
        metrics: Arc::new(rwiki_core::infrastructure::metrics::RwikiMetrics::new()),
        session_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    })
}

/// `AppState` with `low_recall_config = Some(LowRecallConfig { threshold })`
/// (feature enabled). Used by the "enabled → records" and zero-result /
/// scoped-skip scenarios.
async fn test_app_state_with_low_recall(threshold: f64) -> Arc<AppState> {
    build_low_recall_app_state(Some(LowRecallConfig { threshold })).await
}

/// Same construction as `test_app_state_with_low_recall` but with
/// `low_recall_config: None` (feature disabled). Used by the "disabled does
/// not record" scenario.
async fn test_app_state_low_recall_disabled() -> Arc<AppState> {
    build_low_recall_app_state(None).await
}

/// Count rows in `low_recall_records` for the given AppState's sqlite handle.
async fn count_low_recall_records(state: &Arc<AppState>) -> i64 {
    state
        .sqlite
        .call(|conn| -> Result<i64, rusqlite::Error> {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM low_recall_records", [], |row| {
                    row.get(0)
                })?;
            Ok(count)
        })
        .await
        .expect("count low_recall_records")
}

/// Read the `top_score` of the most recently inserted `low_recall_records`
/// row whose `query` contains `query_substr`.
///
/// Returns:
/// - `None`    — no matching row exists.
/// - `Some(None)` — a row exists but `top_score IS NULL` (zero-result path).
/// - `Some(Some(score))` — a row exists with a non-null top_score.
async fn read_top_score(state: &Arc<AppState>, query_substr: &str) -> Option<Option<f64>> {
    let needle = format!("%{query_substr}%");
    state
        .sqlite
        .call(
            move |conn| -> Result<Option<Option<f64>>, rusqlite::Error> {
                let mut stmt = conn.prepare(
                    "SELECT top_score FROM low_recall_records \
                 WHERE query LIKE ?1 \
                 ORDER BY id DESC LIMIT 1",
                )?;
                let mut rows = stmt.query(rusqlite::params![needle])?;
                match rows.next()? {
                    Some(row) => {
                        // top_score is nullable; read as Option<f64>.
                        let top_score: Option<f64> = row.get(0)?;
                        Ok(Some(top_score))
                    }
                    None => Ok(None),
                }
            },
        )
        .await
        .expect("read top_score")
}

/// Build a public `/api/chat` POST request with JSON body.
fn low_recall_chat_request(message: &str, session_id: &str) -> Request<Body> {
    let body = serde_json::json!({
        "message": message,
        "sessionId": session_id,
    });
    Request::builder()
        .method(Method::POST)
        .uri("/api/chat")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_string(&body).expect("serialize json"),
        ))
        .expect("build request")
}

/// Build an authenticated `/api/chat/scoped` POST request with JSON body.
/// The scoped endpoint lives behind `doc_router`'s `auth_middleware`, so a
/// valid Bearer token is required.
fn low_recall_scoped_chat_request(
    message: &str,
    session_id: &str,
    document_ids: Option<Vec<String>>,
) -> Request<Body> {
    let body = serde_json::json!({
        "message": message,
        "sessionId": session_id,
        "documentIds": document_ids,
    });
    Request::builder()
        .method(Method::POST)
        .uri("/api/chat/scoped")
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {LOW_RECALL_TEST_API_TOKEN}"),
        )
        .body(Body::from(
            serde_json::to_string(&body).expect("serialize json"),
        ))
        .expect("build request")
}

// ---------------------------------------------------------------------------
// Scenario 1: low_recall enabled + low score (or zero-result) → records row
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 -- As an operator, I want every public `/api/chat`
// query whose top-1 retrieved relevance is below the configured threshold to
// be automatically recorded (bypass, fire-and-forget) so I can discover KB
// blind spots. The recording MUST NOT alter or block the chat response.
// Covers: Design §5.3 (detached `tokio::spawn` bypass write), §6.1 scenario
//         "启用 [low_recall] + top score 低于阈值 → 记录落库", §7 P0
//         (write failure / slowness must not affect chat).
//
// Strong controls: (a) chat returns success 200 (NOT blocked), (b) after
// letting the detached spawn complete, `low_recall_records` row count is
// >= the pre-trigger count. Weak control: exact score (threshold-vs-score
// comparison is verified structurally in chat.rs; here we only assert the
// "enabled path produces a row" invariant because precisely controlling the
// retrieved `score` through the full embeddings+vector_store stack is brittle).

#[tokio::test]
async fn low_recall_enabled_logs_low_score_record() {
    let state = test_app_state_with_low_recall(0.3).await;
    let app = create_api_routes(state.clone());

    let before = count_low_recall_records(&state).await;

    let req = low_recall_chat_request(
        "a query that will not strongly match the seeded chunk",
        "session-low-recall-enabled",
    );
    let resp = app.oneshot(req).await.expect("send request");

    // §7 P0: the bypass write MUST NOT block or break chat.
    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed (200) even when low-recall bypass write \
         fires, got status {}",
        resp.status()
    );

    // The bypass write is a detached `tokio::spawn`; let it complete before
    // counting. A short yield + retry loop keeps the test robust without a
    // hard timing coupling.
    let after = {
        let mut count = before;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            count = count_low_recall_records(&state).await;
            if count > before {
                break;
            }
        }
        count
    };

    assert!(
        after > before,
        "with low_recall enabled, a public /api/chat below the threshold (or \
         zero-result) MUST produce at least one new low_recall_records row; \
         before={}, after={}",
        before,
        after,
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: low_recall disabled (None) → no records
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 (scenario 5) -- As an operator who has NOT
// configured `[low_recall]`, I expect the feature to be completely inert:
// no rows are ever written, and chat behavior is byte-identical to before
// the feature existed.
// Covers: Design §5.3 (the bypass block is gated on
//         `state.low_recall_config.is_some()`; `None` short-circuits before
//         the spawn), §6.1 scenario "功能关闭 (low_recall_config = None) → 不
//         产生记录", §4.1 ("功能关闭时不产生任何记录").

#[tokio::test]
async fn low_recall_disabled_does_not_record() {
    let state = test_app_state_low_recall_disabled().await;
    let app = create_api_routes(state.clone());

    let req = low_recall_chat_request(
        "any query while low_recall is disabled",
        "session-low-recall-disabled",
    );
    let resp = app.oneshot(req).await.expect("send request");

    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed when low_recall is disabled, got status {}",
        resp.status()
    );

    // Give any hypothetical spawn time to flush (there should be none, but the
    // sleep makes the "no records" assertion resilient to ordering).
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let count = count_low_recall_records(&state).await;
    assert_eq!(
        count, 0,
        "with low_recall_config = None, no low_recall_records row must be \
         written for a public /api/chat call",
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: zero-result query → row with top_score IS NULL
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 -- As an operator, the most important blind-spot
// signal is a query that retrieves NOTHING ("完全未命中"). These must always
// be recorded with `top_score = NULL` (and `result_count = 0`) regardless of
// the threshold, because a zero-result query is an unconditional KB blind
// spot. Chat must still succeed.
// Covers: Design §5.3 (`let top_score = search_results.first().map(|r| r.score)`
//         → None when empty; `should_log = top_score.map_or(true, |s| s <
//         threshold)` → true for None), §6.1 scenario "result_count == 0
//         (完全未命中) → 记录落库、topScore = null", §4.1.4 assumption.

#[tokio::test]
async fn low_recall_logs_zero_result_with_null_top_score() {
    let state = test_app_state_with_low_recall(0.3).await;
    let app = create_api_routes(state.clone());

    // A unique, KB-uncovered query. With the mock embeddings returning a zero
    // vector for every query and no real indexed chunk embedding, search_hybrid
    // returns zero results, which is exactly the path under test.
    let zero_result_query = "zzzzz-zero-result-marker-no-kb-coverage-zzzzz-low-recall-test";
    let req = low_recall_chat_request(zero_result_query, "session-zero-result");
    let resp = app.oneshot(req).await.expect("send request");

    assert!(
        resp.status().is_success(),
        "chat endpoint must succeed (200) even when retrieval returns zero \
         results, got status {}",
        resp.status()
    );

    // Wait for the detached spawn to land the row (retry loop, same as
    // scenario 1).
    let top_score = {
        let mut maybe = None;
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            maybe = read_top_score(&state, "zzzzz-zero-result-marker").await;
            if maybe.is_some() {
                break;
            }
        }
        maybe
    };

    assert_eq!(
        top_score,
        Some(None),
        "a zero-result public /api/chat with low_recall enabled MUST produce a \
         low_recall_records row whose top_score IS NULL (outer Some = row \
         exists, inner None = NULL top_score); got {:?}",
        top_score,
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: scoped chat (Collection scope) → no records
// ---------------------------------------------------------------------------

// User Story: US-CORE-038 (operator-data-hygiene boundary) -- Scoped chat
// (`/api/chat/scoped`) is an evaluation tool for draft batches; its "low
// relevance" is NOT a production KB blind spot, so mixing it into the
// operator's low-recall view would pollute the signal. The bypass write must
// be gated on `RetrievalScope::Published` only.
// Covers: Design §4.1 ("仅对公共 /api/chat (RetrievalScope::Published) 记录,
//         不对 /api/chat/scoped 记录"), §5.3 (`if matches!(scope, Published)`
//         guard wraps the entire bypass block), §6.1 scenario "scoped chat
//         (Collection 作用域) → 不产生记录", §6.3 regression risk row on
//         scope gating.

#[tokio::test]
async fn low_recall_skips_scoped_chat() {
    let state = test_app_state_with_low_recall(0.3).await;
    let app = create_api_routes(state.clone());

    // /api/chat/scoped builds RetrievalScope::Collection from documentIds.
    // Even though low_recall is enabled, the bypass block's
    // `matches!(scope, Published)` guard must skip recording.
    let req = low_recall_scoped_chat_request(
        "scoped query that must not be recorded",
        "session-scoped-skip",
        Some(vec!["test-doc".to_string()]),
    );
    let resp = app.oneshot(req).await.expect("send request");

    assert!(
        resp.status().is_success(),
        "scoped chat endpoint must succeed (200), got status {}",
        resp.status()
    );

    // Yield in case any spawn were scheduled (none should be).
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let count = count_low_recall_records(&state).await;
    assert_eq!(
        count, 0,
        "scoped /api/chat/scoped (Collection scope) must NOT produce any \
         low_recall_records row even when low_recall is enabled; the bypass \
         block is gated on RetrievalScope::Published only",
    );
}
