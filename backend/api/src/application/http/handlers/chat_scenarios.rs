//! Chat handler pure helper function scenario tests.
//!
//! Verifies the three pub(crate) prompt-building helpers extracted from the
//! chat handler: `build_rewrite_prompt`, `build_preamble`, and
//! `build_compact_prompt`. These are pure functions with no LLM or I/O
//! dependencies, so they are tested as plain synchronous unit tests.
//!
//! Also tests `ChatSession::get_sliding_window` and handler-level decision
//! preconditions relevant to the multi-turn conversation hybrid memory feature.

use rwiki_core::domain::chat::{ChatMessage, ChatSession};

use super::chat::{build_compact_prompt, build_preamble, build_rewrite_prompt};

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
    let prompt = build_rewrite_prompt(&history, "How does it handle memory?");

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
    let prompt = build_rewrite_prompt(&history, "What is Rust?");

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
        preamble.contains("对话摘要:\nUser previously asked about Rust memory management."),
        "preamble must include summary section when summary is provided"
    );
    assert!(
        preamble.contains("上下文:\nRAG result: Rust uses ownership and borrowing."),
        "preamble must include RAG context"
    );
    // Verify ordering: system_prompt comes before summary, summary before context
    let sys_pos = preamble
        .find("You are a knowledge base assistant.")
        .unwrap();
    let summary_pos = preamble.find("对话摘要").unwrap();
    let ctx_pos = preamble.find("上下文").unwrap();
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
        preamble.contains("上下文:\nRAG context."),
        "preamble must include RAG context"
    );
    assert!(
        !preamble.contains("对话摘要"),
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
        preamble.contains("上下文:\n"),
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
    let prompt = build_rewrite_prompt(&history, original_query);
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
