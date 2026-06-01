//! Scenario tests for ChatSession context management.
//!
//! Covers session creation, message accumulation, history retrieval,
//! stable ID across operations, concurrent access safety,
//! sliding window, token estimation, and compact history.
//!
//! These are pure in-memory tests with no external dependencies.

use super::chat::ChatSession;

// ---------------------------------------------------------------------------
// Session creation
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: ChatSession::new generates a session with the given ID and empty messages.
#[test]
fn new_session_has_given_id_and_empty_messages() {
    let id = "test-session-001".to_string();
    let session = ChatSession::new(id.clone());

    assert_eq!(session.id, id, "session ID should match the given ID");
    assert!(
        session.messages.is_empty(),
        "new session should have no messages"
    );
}

// User Story: US-CORE-002
// Covers: ChatSession::new generates unique IDs when called with different values.
#[test]
fn new_session_with_different_ids_produces_distinct_sessions() {
    let session_a = ChatSession::new("session-a".to_string());
    let session_b = ChatSession::new("session-b".to_string());

    assert_ne!(
        session_a.id, session_b.id,
        "sessions with different IDs should be distinct"
    );
}

// ---------------------------------------------------------------------------
// Message accumulation
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: ChatSession::add_message appends user and assistant messages in order.
#[test]
fn add_message_appends_user_and_assistant_messages() {
    let mut session = ChatSession::new("s1".to_string());

    session.add_message("user", "Hello");
    session.add_message("assistant", "Hi there!");

    assert_eq!(session.messages.len(), 2, "should have 2 messages");

    assert_eq!(session.messages[0].role, "user");
    assert_eq!(session.messages[0].content, "Hello");

    assert_eq!(session.messages[1].role, "assistant");
    assert_eq!(session.messages[1].content, "Hi there!");
}

// User Story: US-CORE-002
// Covers: Multiple add_message calls accumulate correctly; messages are never lost.
#[test]
fn multiple_add_message_calls_accumulate_correctly() {
    let mut session = ChatSession::new("s2".to_string());

    for i in 0..10 {
        session.add_message("user", format!("Q{i}"));
        session.add_message("assistant", format!("A{i}"));
    }

    assert_eq!(
        session.messages.len(),
        20,
        "should have 20 messages (10 pairs)"
    );

    // Verify order is preserved
    for i in 0..10 {
        let user_msg = &session.messages[i * 2];
        let assistant_msg = &session.messages[i * 2 + 1];
        assert_eq!(user_msg.role, "user");
        assert_eq!(user_msg.content, format!("Q{i}"));
        assert_eq!(assistant_msg.role, "assistant");
        assert_eq!(assistant_msg.content, format!("A{i}"));
    }
}

// ---------------------------------------------------------------------------
// History retrieval
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: ChatSession::get_history returns messages in insertion order.
#[test]
fn get_history_returns_messages_in_insertion_order() {
    let mut session = ChatSession::new("s3".to_string());
    session.add_message("user", "first");
    session.add_message("assistant", "second");
    session.add_message("user", "third");

    let history = session.get_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].content, "first");
    assert_eq!(history[1].content, "second");
    assert_eq!(history[2].content, "third");
}

// User Story: US-CORE-002
// Covers: get_history on empty session returns empty slice.
#[test]
fn get_history_on_empty_session_returns_empty_slice() {
    let session = ChatSession::new("s4".to_string());
    assert!(session.get_history().is_empty());
}

// ---------------------------------------------------------------------------
// Stable ID
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: Session ID remains stable across add_message and get_history operations.
#[test]
fn session_id_remains_stable_across_operations() {
    let expected_id = "stable-id-test".to_string();
    let mut session = ChatSession::new(expected_id.clone());

    session.add_message("user", "msg1");
    let _ = session.get_history();
    session.add_message("assistant", "msg2");
    let _ = session.get_history();

    assert_eq!(
        session.id, expected_id,
        "session ID must not change after operations"
    );
}

// ---------------------------------------------------------------------------
// Concurrent access
// ---------------------------------------------------------------------------

// User Story: US-CORE-002
// Covers: Multiple concurrent add_message calls via tokio tasks do not lose messages.
//         Tests that a single ChatSession can handle concurrent writes safely.
//         Note: ChatSession itself is not Sync, so we test via Arc<Mutex<ChatSession>>.
//         This test is #[ignore] because it requires tokio runtime and simulates
//         the Mutex pattern used in AppState's chat_sessions field.
#[tokio::test]
#[ignore = "requires tokio runtime; validates Mutex concurrency pattern"]
async fn concurrent_add_message_does_not_lose_messages() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let session = Arc::new(Mutex::new(ChatSession::new("concurrent-test".to_string())));
    let mut handles = Vec::new();

    for i in 0..10 {
        let s = Arc::clone(&session);
        handles.push(tokio::spawn(async move {
            let mut guard = s.lock().await;
            guard.add_message("user", format!("msg-{i}"));
        }));
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("task should complete");
    }

    let guard = session.lock().await;
    assert_eq!(
        guard.messages.len(),
        10,
        "all 10 messages should be present after concurrent adds"
    );
}

// ---------------------------------------------------------------------------
// Summary field
// ---------------------------------------------------------------------------

// Covers: New session has summary = None.
#[test]
fn new_session_has_summary_none() {
    let session = ChatSession::new("summary-test".to_string());
    assert!(
        session.summary.is_none(),
        "new session should have summary = None"
    );
}

// ---------------------------------------------------------------------------
// Sliding window
// ---------------------------------------------------------------------------

// Covers: get_sliding_window returns last N messages when more than N exist.
#[test]
fn sliding_window_returns_last_n_messages() {
    let mut session = ChatSession::new("sw1".to_string());
    for i in 0..10 {
        session.add_message("user", format!("msg{i}"));
    }

    let window = session.get_sliding_window(3);
    assert_eq!(window.len(), 3);
    assert_eq!(window[0].content, "msg7");
    assert_eq!(window[1].content, "msg8");
    assert_eq!(window[2].content, "msg9");
}

// Covers: get_sliding_window returns all messages when fewer than window_size.
#[test]
fn sliding_window_returns_all_when_fewer_than_window_size() {
    let mut session = ChatSession::new("sw2".to_string());
    session.add_message("user", "a");
    session.add_message("user", "b");

    let window = session.get_sliding_window(5);
    assert_eq!(window.len(), 2, "should return all 2 messages");
    assert_eq!(window[0].content, "a");
    assert_eq!(window[1].content, "b");
}

// Covers: get_sliding_window on empty session returns empty slice.
#[test]
fn sliding_window_on_empty_session_returns_empty() {
    let session = ChatSession::new("sw3".to_string());
    let window = session.get_sliding_window(3);
    assert!(window.is_empty());
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

// Covers: estimate_tokens returns expected approximation with known char counts.
#[test]
fn estimate_tokens_returns_expected_approximation() {
    let mut session = ChatSession::new("tok1".to_string());
    // Each message has 10 chars = 20 total message chars
    session.add_message("user", "1234567890");
    session.add_message("assistant", "abcdefghij");

    // No summary: 20 chars * 3 / 5 = 12 tokens
    assert_eq!(session.estimate_tokens(), 12);
}

// Covers: estimate_tokens includes summary chars in the calculation.
#[test]
fn estimate_tokens_includes_summary() {
    let mut session = ChatSession::new("tok2".to_string());
    session.summary = Some("1234567890".to_string()); // 10 chars
    session.add_message("user", "12345"); // 5 chars

    // (10 + 5) * 3 / 5 = 9 tokens
    assert_eq!(session.estimate_tokens(), 9);
}

// Covers: estimate_tokens handles Chinese characters correctly via chars().count().
#[test]
fn estimate_tokens_handles_chinese_chars() {
    let mut session = ChatSession::new("tok3".to_string());
    // 3 Chinese chars, each 3 bytes in UTF-8 but 1 char
    session.add_message("user", "你好吗");

    // 3 chars * 3 / 5 = 1 (integer division)
    assert_eq!(session.estimate_tokens(), 1);
}

// ---------------------------------------------------------------------------
// Should compact
// ---------------------------------------------------------------------------

// Covers: should_compact returns false when messages below threshold.
#[test]
fn should_compact_returns_false_below_threshold() {
    let mut session = ChatSession::new("sc1".to_string());
    for i in 0..5 {
        session.add_message("user", format!("m{i}"));
    }

    assert!(
        !session.should_compact(10, 8000, 6),
        "5 messages < threshold 10, should not compact"
    );
}

// Covers: should_compact returns true when messages exceed threshold.
#[test]
fn should_compact_returns_true_above_threshold() {
    let mut session = ChatSession::new("sc2".to_string());
    for i in 0..12 {
        session.add_message("user", format!("m{i}"));
    }

    assert!(
        session.should_compact(10, 8000, 6),
        "12 messages > threshold 10 and > sliding_window 6, should compact"
    );
}

// Covers: should_compact returns true when tokens exceed budget
// even though message count is below threshold.
#[test]
fn should_compact_returns_true_when_tokens_exceed_budget() {
    let mut session = ChatSession::new("sc3".to_string());
    // 4 messages, each 2000 chars = 8000 chars total => 8000*3/5 = 4800 tokens
    // Below threshold but we set a very low token_budget to trigger
    for _ in 0..4 {
        session.add_message("user", "a".repeat(2000));
    }

    // 4 messages, threshold=10 (not exceeded), token_budget=100 (exceeded)
    // 4 > sliding_window_size=2, so there are messages outside window
    assert!(
        session.should_compact(10, 100, 2),
        "tokens exceed budget and messages outside window, should compact"
    );
}

// Covers: should_compact returns false when messages exceed threshold
// but all messages fit within sliding window (no messages to compact out).
#[test]
fn should_compact_returns_false_when_all_within_sliding_window() {
    let mut session = ChatSession::new("sc4".to_string());
    // 8 messages, threshold=5 (exceeded), sliding_window_size=10
    // All 8 messages fit within sliding window of 10, nothing to compact out
    for i in 0..8 {
        session.add_message("user", format!("m{i}"));
    }

    assert!(
        !session.should_compact(5, 8000, 10),
        "8 > threshold 5 but 8 <= sliding_window 10, no messages outside window"
    );
}

// ---------------------------------------------------------------------------
// Compact history
// ---------------------------------------------------------------------------

// Covers: compact_history retains last window_size messages and sets summary.
#[test]
fn compact_history_retains_window_and_sets_summary() {
    let mut session = ChatSession::new("ch1".to_string());
    for i in 0..10 {
        session.add_message("user", format!("msg{i}"));
    }

    session.compact_history("Summary of old messages".to_string(), 3);

    assert_eq!(session.messages.len(), 3, "should retain 3 messages");
    assert_eq!(session.messages[0].content, "msg7");
    assert_eq!(session.messages[1].content, "msg8");
    assert_eq!(session.messages[2].content, "msg9");
    assert_eq!(session.summary, Some("Summary of old messages".to_string()));
}

// Covers: compact_history replaces existing summary.
#[test]
fn compact_history_replaces_existing_summary() {
    let mut session = ChatSession::new("ch2".to_string());
    session.summary = Some("Old summary".to_string());
    for i in 0..10 {
        session.add_message("user", format!("msg{i}"));
    }

    session.compact_history("New summary".to_string(), 3);

    assert_eq!(
        session.summary,
        Some("New summary".to_string()),
        "summary should be replaced"
    );
}

// Covers: After compact, sliding window messages are still accessible.
#[test]
fn after_compact_sliding_window_still_accessible() {
    let mut session = ChatSession::new("ch3".to_string());
    for i in 0..10 {
        session.add_message("user", format!("msg{i}"));
    }

    session.compact_history("Summary".to_string(), 4);

    let window = session.get_sliding_window(4);
    assert_eq!(window.len(), 4);
    assert_eq!(window[0].content, "msg6");
    assert_eq!(window[3].content, "msg9");
}

// Covers: compact_history with messages <= window_size keeps all messages.
#[test]
fn compact_history_keeps_all_when_within_window() {
    let mut session = ChatSession::new("ch4".to_string());
    session.add_message("user", "a");
    session.add_message("user", "b");

    session.compact_history("Short summary".to_string(), 5);

    assert_eq!(session.messages.len(), 2, "should keep all 2 messages");
    assert_eq!(session.summary, Some("Short summary".to_string()));
}
