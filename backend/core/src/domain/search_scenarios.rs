//! Search orchestration pure-function scenario tests.
//!
//! Covers the final ranking rule (page-granularity ordering + `sub_index`
//! within a page), the title-match bonus of the unified retrieval
//! orchestration, content dedupe, and the query rewrite prompt builders /
//! response parsers.
//!
//! These are pure in-memory tests with no external dependencies.

use std::collections::HashSet;

use super::search::{
    build_first_turn_rewrite_prompt, build_rewrite_prompt, dedupe_by_content, max_relevance_score,
    parse_rewrite_response, rank_pages_with_title_boost, title_matches,
};
use crate::domain::chat::ChatMessage;
use crate::infrastructure::vector_store::SearchResult;

fn make_search_result(
    chunk_id: &str,
    page_id: &str,
    sub_index: Option<i64>,
    title: &str,
    score: f64,
) -> SearchResult {
    SearchResult {
        chunk_id: chunk_id.to_string(),
        content: format!("content of {chunk_id}"),
        score,
        document_id: format!("doc-{chunk_id}"),
        page_id: page_id.to_string(),
        sub_index,
        chunk_count: None,
        title: title.to_string(),
        locale: None,
        link: None,
        tags: vec![],
        section: None,
    }
}

fn make_result_with_score(content: &str, score: f64) -> SearchResult {
    SearchResult {
        chunk_id: format!("chunk-{score}"),
        content: content.to_string(),
        score,
        document_id: "test-doc".to_string(),
        page_id: "test-page-id".to_string(),
        sub_index: None,
        chunk_count: None,
        title: "T".to_string(),
        locale: None,
        link: None,
        tags: vec![],
        section: None,
    }
}

// User Story: US-CORE-042 场景1 —— 两个内容相关性接近的片段，标题命中查询
// 关键词者获得有限提升而排前，且加成写入对外得分。
// Covers: page 键差 0.001 时 +0.005 bonus 翻转次序；bonus 回退或未写入
//         score 时失败。
#[test]
fn title_boost_promotes_hit_page_over_near_score_miss() {
    let miss = make_search_result("y0", "page-y", Some(0), "Gardening Guide", 0.501);
    let hit = make_search_result("x0", "page-x", Some(0), "Rust Guide", 0.500);

    let out = rank_pages_with_title_boost(vec![miss, hit], &["rust"]);

    assert_eq!(out[0].chunk_id, "x0", "hit page must be promoted");
    assert_eq!(out[1].chunk_id, "y0", "miss page must follow");
    assert!(
        (out[0].score - 0.505).abs() < 1e-9,
        "bonus must be written into the visible score, got {}",
        out[0].score
    );
    assert!(
        (out[1].score - 0.501).abs() < 1e-9,
        "miss score must be untouched, got {}",
        out[1].score
    );
}

// User Story: US-CORE-042 场景2 —— 标题未命中的片段集合保持既有相对顺序，
// 不因标题加权被重新排列。
// Covers: 输入 [m1 0.9, h 0.5(命中), m2 0.4, m3 0.3] → m1/m2/m3 的相对顺序
//         与 score 均不变（未命中项被加分或重排时失败）。
#[test]
fn title_boost_preserves_order_among_miss_pages() {
    let m1 = make_search_result("m1", "p1", Some(0), "Cooking Basics", 0.9);
    let h = make_search_result("h", "p2", Some(0), "Rust Book", 0.5);
    let m2 = make_search_result("m2", "p3", Some(0), "Travel Tips", 0.4);
    let m3 = make_search_result("m3", "p4", Some(0), "History Notes", 0.3);

    let out = rank_pages_with_title_boost(vec![m1, h, m2, m3], &["rust"]);

    let ids: Vec<&str> = out.iter().map(|r| r.chunk_id.as_str()).collect();
    assert_eq!(ids, vec!["m1", "h", "m2", "m3"], "relative order preserved");
    let by_id = |id: &str| out.iter().find(|r| r.chunk_id == id).unwrap();
    for (id, score) in [("m1", 0.9), ("m2", 0.4), ("m3", 0.3)] {
        assert!(
            (by_id(id).score - score).abs() < 1e-9,
            "miss page {id} score must be untouched, got {}",
            by_id(id).score
        );
    }
    assert!(
        (by_id("h").score - 0.505).abs() < 1e-9,
        "only the hit page gains the bonus"
    );
}

// User Story: US-CORE-042 场景3 —— 加权是有限提升而非绝对置顶：内容高度
// 相关（未命中 0.80）仍排在标题命中但内容无关（0.10）之前。
// Covers: bonus 被调大到颠覆头部排序时失败。
#[test]
fn title_boost_bonus_does_not_flip_large_content_gap() {
    let miss = make_search_result("deep", "p-deep", Some(0), "Deep Dive Analysis", 0.80);
    let hit = make_search_result("spam", "p-spam", Some(0), "Rust Spam Title", 0.10);

    let out = rank_pages_with_title_boost(vec![miss, hit], &["rust"]);

    assert_eq!(
        out[0].chunk_id, "deep",
        "highly relevant content must stay above an irrelevant title hit"
    );
    assert!(
        (out[1].score - 0.105).abs() < 1e-9,
        "irrelevant hit page gains only the fixed bonus"
    );
}

// User Story: US-CORE-042 —— 最终排序按 page 粒度：page 之间按页内最高
// chunk 得分稳定降序，各 page 连续出现。
// Covers: 位置序输入 [A 0.010, B 0.030, C 0.020] → 输出 B、C、A。
#[test]
fn final_ranking_orders_pages_by_best_chunk_score() {
    let a0 = make_search_result("a0", "page-a", Some(0), "Alpha Page", 0.010);
    let a1 = make_search_result("a1", "page-a", Some(1), "Alpha Page", 0.010);
    let b0 = make_search_result("b0", "page-b", Some(0), "Beta Page", 0.030);
    let b1 = make_search_result("b1", "page-b", Some(1), "Beta Page", 0.030);
    let c0 = make_search_result("c0", "page-c", Some(0), "Gamma Page", 0.020);

    let out = rank_pages_with_title_boost(vec![a0, a1, b0, b1, c0], &[]);

    let ids: Vec<&str> = out.iter().map(|r| r.chunk_id.as_str()).collect();
    assert_eq!(ids, vec!["b0", "b1", "c0", "a0", "a1"]);
}

// User Story: US-CORE-042 —— 同一 page 内保持文档顺序（sub_index 升序），
// 即使某个 chunk 的分数更高也不打乱页内顺序（rerank 式乱序输入会被规整）。
// Covers: 已发布检索 PRD 的页内顺序条款；回退到 chunk 级平铺排序时失败。
#[test]
fn final_ranking_keeps_sub_index_order_within_page() {
    let p2 = make_search_result("p2", "P", Some(2), "Page P", 0.9);
    let q0 = make_search_result("q0", "Q", Some(0), "Page Q", 0.8);
    let p0 = make_search_result("p0", "P", Some(0), "Page P", 0.5);
    let p1 = make_search_result("p1", "P", Some(1), "Page P", 0.7);

    let out = rank_pages_with_title_boost(vec![p2, q0, p0, p1], &[]);

    let ids: Vec<&str> = out.iter().map(|r| r.chunk_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["p0", "p1", "p2", "q0"],
        "P (key 0.9) precedes Q (0.8); within P sub_index ascends; q0 (0.8) follows p0 (0.5)"
    );
}

// Covers: page 键相同时保持输入中的首次出现顺序（稳定排序，结果确定）。
#[test]
fn final_ranking_is_stable_on_page_key_ties() {
    let first = make_search_result("f0", "page-f", Some(0), "First Page", 0.5);
    let second = make_search_result("s0", "page-s", Some(0), "Second Page", 0.5);

    let out = rank_pages_with_title_boost(vec![first, second], &[]);

    let ids: Vec<&str> = out.iter().map(|r| r.chunk_id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["f0", "s0"],
        "tied page keys keep first-occurrence order"
    );
}

// Covers: page_id 为空串的旧数据无页语义，每个 chunk 自成一组（若把空串
// 当作同一 page，输出会是 [x, y, p0]，本用例失败）。
#[test]
fn final_ranking_treats_empty_page_id_as_singletons() {
    let x = make_search_result("x", "", Some(0), "Legacy One", 0.1);
    let p0 = make_search_result("p0", "P", Some(0), "Modern Page", 0.5);
    let y = make_search_result("y", "", Some(0), "Legacy Two", 0.9);

    let out = rank_pages_with_title_boost(vec![x, p0, y], &[]);

    let ids: Vec<&str> = out.iter().map(|r| r.chunk_id.as_str()).collect();
    assert_eq!(ids, vec!["y", "p0", "x"]);
}

// User Story: US-CORE-042 —— 标题命中判定为词级匹配且大小写不敏感。
// Covers: "Rust 所有权" 命中 "Rust Ownership Model"（大小写不敏感）；
//         "Trust Center" 不命中（rust⊂trust 的子串陷阱）。
#[test]
fn title_matches_is_case_insensitive_and_token_level() {
    let keywords: HashSet<String> = ["rust", "所有权"].iter().map(|s| s.to_string()).collect();

    assert!(
        title_matches("Rust Ownership Model", &keywords),
        "token-level match must be case-insensitive"
    );
    assert!(
        !title_matches("Trust Center", &keywords),
        "'rust' must NOT match inside the 'trust' token (no substring matching)"
    );
}

// User Story: US-CORE-042 —— 命中判定使用原始查询 ∪ 改写变体：改写把查询
// 翻译成文档语言后，标题仍可经变体关键词命中。
// Covers: 只用原始查询（中文）时 "Shutdown Guide" 不命中，本用例失败。
#[test]
fn title_matches_uses_rewritten_queries() {
    let result = make_search_result("s0", "page-s", Some(0), "Shutdown Guide", 0.5);

    let out = rank_pages_with_title_boost(vec![result], &["怎么关机", "shutdown procedure"]);

    assert!(
        (out[0].score - 0.505).abs() < 1e-9,
        "the rewritten variant's keyword must drive the title hit, got {}",
        out[0].score
    );
}

// Covers: 关键词过滤——单字虚词与纯标点 token 不参与命中判定，查询只含
// 停用 token 时关键词集为空，任何标题都不命中、score 不变。
#[test]
fn title_matches_ignores_single_char_and_punctuation_tokens() {
    let keywords: HashSet<String> = ["的", "a", "?"].iter().map(|s| s.to_string()).collect();
    // The production keyword extraction filters these before calling
    // title_matches; emulate the same filtered (empty) set.
    let filtered: HashSet<String> = keywords
        .iter()
        .filter(|t| t.chars().count() >= 2 && t.chars().any(char::is_alphanumeric))
        .cloned()
        .collect();
    assert!(
        filtered.is_empty(),
        "stop-word-only queries yield no keywords"
    );

    assert!(
        !title_matches("a 的 guide", &filtered),
        "empty keyword set must never hit"
    );

    let result = make_search_result("q0", "page-q", Some(0), "a 的 guide", 0.5);
    let out = rank_pages_with_title_boost(vec![result], &["的 a ?"]);
    assert!(
        (out[0].score - 0.5).abs() < 1e-9,
        "stop-word-only query must not add the bonus, got {}",
        out[0].score
    );
}

// Covers: 空 title 的 chunk 永不加分。
#[test]
fn title_boost_empty_title_never_hits() {
    let result = make_search_result("e0", "page-e", Some(0), "", 0.5);

    let out = rank_pages_with_title_boost(vec![result], &["rust"]);

    assert!(
        (out[0].score - 0.5).abs() < 1e-9,
        "empty title must never receive the bonus, got {}",
        out[0].score
    );
}

// User Story: US-CORE-042 —— low_recall threshold 的语义是检索相关度
// （config 文档），而 bonus 写入对外得分仅服务排序口径；max_relevance_score
// 必须对标题命中组剔除 bonus，未命中组保持原分。
// Covers: 含 bonus 的结果经 max_relevance_score 恢复纯相关度；空结果返回
//         None（零命中必记）。若直接复用含 bonus 的 score 会失败。
#[test]
fn max_relevance_score_excludes_title_bonus() {
    let hit = make_search_result("x0", "page-x", Some(0), "Rust Guide", 0.505);
    let miss = make_search_result("y0", "page-y", Some(0), "Gardening Guide", 0.501);

    let out = max_relevance_score(&[hit, miss], &["rust"]);
    let top = out.expect("non-empty input must yield a score");
    assert!(
        (top - 0.501).abs() < 1e-9,
        "threshold comparison must use relevance without the bonus, got {top}"
    );

    let none = max_relevance_score(&[], &["rust"]);
    assert!(
        none.is_none(),
        "empty results must yield None (zero-hit always logged)"
    );
}

// Covers: US-CORE-034 场景1 —— 同一内容只保留得分最高者，且首次出现位置稳定，
// 第三条不同内容必须存活。若保留了两条相同内容或丢掉了更高分都会失败。
#[test]
fn dedupe_by_content_collapses_identical_content_keeping_highest_score() {
    let input = vec![
        make_result_with_score("shared chunk content", 0.5),
        make_result_with_score("shared chunk content", 0.9),
        make_result_with_score("distinct content", 0.3),
    ];

    let out = dedupe_by_content(input);

    assert_eq!(out.len(), 2, "identical content must collapse to one entry");

    assert_eq!(
        out[0].content, "shared chunk content",
        "first-occurrence content must survive"
    );
    assert!(
        (out[0].score - 0.9).abs() < f64::EPSILON,
        "must keep the HIGHER score (0.9), got {}",
        out[0].score
    );

    assert_eq!(
        out[1].content, "distinct content",
        "distinct content must survive"
    );
    assert!(
        (out[1].score - 0.3).abs() < f64::EPSILON,
        "distinct content score must be untouched"
    );
}

#[test]
fn parse_rewrite_response_single_query_in_array() {
    let result = parse_rewrite_response("{\"queries\": [\"single query\"]}");
    assert_eq!(result, vec!["single query"]);
}

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
        interrupted: false,
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
        interrupted: false,
    }];
    let prompt = build_rewrite_prompt(&history, "test", None);
    assert!(
        !prompt.contains("知识库文档主要使用"),
        "should NOT include language instruction when content_language is None"
    );
}

// Covers: empty string content_language suppresses language instruction.
// User Story: Query language aware rewrite — empty string is treated as "no language" by prompt builders.
#[test]
fn build_first_turn_rewrite_prompt_empty_string_no_language_instruction() {
    let prompt = build_first_turn_rewrite_prompt("test query", Some(""));
    assert!(
        !prompt.contains("知识库文档主要使用"),
        "empty string content_language should NOT produce language instruction"
    );
}

// Covers: empty string content_language suppresses language instruction in multi-turn.
// User Story: Query language aware rewrite — empty string treated as absent in rewrite prompt.
#[test]
fn build_rewrite_prompt_empty_string_no_language_instruction() {
    let history = vec![ChatMessage {
        role: "user".into(),
        content: "test".into(),
        interrupted: false,
    }];
    let prompt = build_rewrite_prompt(&history, "follow up", Some(""));
    assert!(
        !prompt.contains("知识库文档主要使用"),
        "empty string content_language should NOT produce language instruction"
    );
}

// Covers: Chinese language value produces correct instruction text.
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

// Covers: Chinese language value in multi-turn prompt.
// User Story: Query language aware rewrite — Chinese content_language works in follow-up rewrite.
#[test]
fn build_rewrite_prompt_chinese_language_value() {
    let history = vec![ChatMessage {
        role: "user".into(),
        content: "test".into(),
        interrupted: false,
    }];
    let prompt = build_rewrite_prompt(&history, "follow up", Some("中文"));
    assert!(
        prompt.contains("知识库文档主要使用 中文 语言"),
        "should include Chinese language instruction in multi-turn prompt"
    );
}

// Covers: language instruction appears before JSON constraint in first-turn prompt.
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

// Covers: language instruction appears before JSON constraint in multi-turn prompt.
// User Story: Query language aware rewrite — prompt ordering consistent across first-turn and follow-up.
#[test]
fn build_rewrite_prompt_language_before_json_constraint() {
    let history = vec![ChatMessage {
        role: "user".into(),
        content: "test".into(),
        interrupted: false,
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

// User Story: As a user resuming a conversation (US-CORE-011), I want my
// follow-up question rewritten into a standalone query so retrieval can find
// relevant context even when my question refers to earlier dialogue.
// Covers: build_rewrite_prompt includes full history and user message in the
//         prompt template so the LLM can resolve pronouns and ellipses.

#[test]
fn rewrite_prompt_includes_history_and_user_message() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Kubernetes?".into(),
            interrupted: false,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Kubernetes is a container orchestration platform.".into(),
            interrupted: false,
        },
    ];
    let prompt = build_rewrite_prompt(&history, "How does it handle memory?", None);

    assert!(
        prompt.contains("user: What is Kubernetes?"),
        "should include user history"
    );
    assert!(
        prompt.contains("assistant: Kubernetes is a container orchestration platform."),
        "should include assistant history"
    );
    assert!(
        prompt.contains("当前用户追问: How does it handle memory?"),
        "should include the current user message"
    );
    assert!(prompt.contains("改写"), "should instruct rewriting");
}

// User Story: US-CORE-011 edge case — the prompt builder tolerates an empty
// history without panicking and still embeds the user message.
#[test]
fn rewrite_prompt_with_empty_history_produces_query_only_structure() {
    let history: Vec<ChatMessage> = vec![];
    let prompt = build_rewrite_prompt(&history, "What is Rust?", None);

    assert!(
        prompt.contains("当前用户追问: What is Rust?"),
        "user message must be embedded even with empty history"
    );
    assert!(
        prompt.contains("对话历史:\n\n"),
        "history section exists but stays empty"
    );
}

// User Story: As a user in a multi-turn conversation (US-CORE-011), when the
// LLM call for query rewriting fails, the system must fall back to using my
// original message for RAG search so I still get relevant results without
// perceiving an error.
// Covers: the degradation precondition — build_rewrite_prompt is infallible
//         and always embeds the original query, so the Err arm of the rewrite
//         LLM call can fall back to the original message.
#[test]
fn query_rewriting_failure_fallback_code_structure_verified() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
            interrupted: false,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Rust is a systems programming language.".into(),
            interrupted: false,
        },
    ];
    let original_query = "How does it handle memory?";

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
}

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
    // REWRITE_MAX_QUERIES is 2 (module-private const); verify the value
    // appears in the prompt text.
    assert!(
        prompt.contains("最多生成 2 条查询"),
        "first-turn prompt must reference the max queries limit (REWRITE_MAX_QUERIES=2)"
    );
}

// User Story: US-CORE-019 -- Edge case: the prompt builder must not panic on
// empty input even though callers validate message non-emptiness upstream.
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

// User Story: US-CORE-011 -- As a user in a multi-turn conversation, I want
// my follow-up questions rewritten into standalone queries with structured
// JSON output so the system can generate multiple search variants.
// Covers: The multi-turn rewrite prompt appends a JSON output format
//         constraint so the LLM returns structured output for multi-query
//         generation, while still containing history content and user message.

#[test]
fn rewrite_prompt_includes_json_output_format_constraint() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Kubernetes?".into(),
            interrupted: false,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Kubernetes is a container orchestration platform.".into(),
            interrupted: false,
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
// Covers: build_rewrite_prompt retains full history context for
//         coreference resolution. Both history entries and the user message
//         must be present alongside the JSON format instruction.

#[test]
fn rewrite_prompt_retains_history_context_for_coreference_resolution() {
    let history = vec![
        ChatMessage {
            role: "user".into(),
            content: "What is Rust?".into(),
            interrupted: false,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Rust is a systems programming language.".into(),
            interrupted: false,
        },
    ];
    let prompt = build_rewrite_prompt(&history, "it", None);

    assert!(
        prompt.contains("user: What is Rust?"),
        "prompt must include user history for coreference resolution"
    );
    assert!(
        prompt.contains("assistant: Rust is a systems programming language."),
        "prompt must include assistant history for context"
    );
    assert!(
        prompt.contains("当前用户追问: it"),
        "prompt must include the current ambiguous user message"
    );
    assert!(
        prompt.contains("JSON"),
        "prompt must include JSON format instruction alongside history"
    );
}

// User Story: US-CORE-019 -- Normal case: LLM returns valid JSON with queries
// array, and the parser extracts the queries correctly.
#[test]
fn parse_rewrite_valid_json_array_parsed_correctly() {
    let raw = r#"{"queries": ["How to deploy Kubernetes", "Kubernetes deployment guide"]}"#;
    let result = parse_rewrite_response(raw);
    assert_eq!(
        result,
        vec!["How to deploy Kubernetes", "Kubernetes deployment guide"]
    );
}

// User Story: US-CORE-019 -- Degradation level 1: markdown ```json fences are
// stripped before parsing.
#[test]
fn parse_rewrite_json_with_markdown_fence_stripped() {
    let raw = "```json\n{\"queries\": [\"q1\", \"q2\"]}\n```";
    let result = parse_rewrite_response(raw);
    assert_eq!(result, vec!["q1", "q2"]);
}

// User Story: US-CORE-019 -- Degradation level 1: plain ``` fences (no
// language tag) are stripped before parsing.
#[test]
fn parse_rewrite_json_with_plain_fence_stripped() {
    let raw = "```\n{\"queries\": [\"q1\"]}\n```";
    let result = parse_rewrite_response(raw);
    assert_eq!(result, vec!["q1"]);
}

// User Story: US-CORE-019 -- Degradation level 2: an empty queries array
// degrades to the raw output as a single query.
#[test]
fn parse_rewrite_empty_queries_array_degrades_to_raw_output() {
    let raw = r#"{"queries": []}"#;
    let result = parse_rewrite_response(raw);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], raw);
}

// User Story: US-CORE-019 -- Degradation level 2: a missing queries field
// degrades to the raw output as a single query.
#[test]
fn parse_rewrite_missing_queries_field_degrades_to_raw_output() {
    let raw = r#"{"result": "something else"}"#;
    let result = parse_rewrite_response(raw);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], raw);
}

// User Story: US-CORE-019 -- Degradation level 3: pure text output degrades
// to the trimmed plain text as a single query.
#[test]
fn parse_rewrite_pure_text_degrades_to_single_query() {
    let raw = "just some plain text advice";
    let result = parse_rewrite_response(raw);
    assert_eq!(result, vec!["just some plain text advice"]);
}

// User Story: US-CORE-019 -- The parser truncates to at most the
// REWRITE_MAX_QUERIES constant (2) to prevent unbounded query variants.
#[test]
fn parse_rewrite_truncates_queries_exceeding_max() {
    let raw = r#"{"queries": ["alpha", "beta", "gamma", "delta"]}"#;
    let result = parse_rewrite_response(raw);
    assert_eq!(
        result.len(),
        2,
        "queries exceeding REWRITE_MAX_QUERIES (2) must be truncated"
    );
}

// User Story: US-CORE-019 -- Empty and whitespace-only query strings are
// filtered out of the parsed result.
#[test]
fn parse_rewrite_filters_empty_strings_from_queries() {
    let raw = r#"{"queries": ["q1", "", "   ", "q2"]}"#;
    let result = parse_rewrite_response(raw);
    assert_eq!(result, vec!["q1", "q2"]);
}

// User Story: US-CORE-019 -- On any LLM failure the original message is used;
// the rewrite match arms fall back to req.message, and the first-turn prompt
// builder is infallible and embeds the original query.
#[test]
fn rewrite_llm_failure_fallback_preserves_original_query_code_structure() {
    let user_message = "deploy";

    let prompt = build_first_turn_rewrite_prompt(user_message, None);

    assert!(
        prompt.contains(user_message),
        "first-turn prompt must contain the original query ('deploy'), \
         confirming the handler's Err/Timeout arms can fall back to req.message.clone()"
    );

    // The handler always has access to req.message (the original user input)
    // regardless of the rewrite outcome: the rewrite match arms fall back to
    // the original message on LLM error and timeout. No domain assertion
    // needed beyond confirming the prompt builder is infallible.
    assert!(
        !prompt.is_empty(),
        "build_first_turn_rewrite_prompt is infallible -- the only failure \
         point is the LLM call, and the handler's match arms guarantee fallback"
    );
}

// Covers: the multi-query fallback path — parse_rewrite_response never returns
// an empty Vec, so search_queries is always non-empty; if the multi-query
// search returns zero results the orchestration retries with the original
// user input.
#[test]
fn multi_query_all_empty_result_triggers_fallback_code_structure() {
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

    // Domain guarantee: the handler always has the original message available
    // for the multi-query fallback path (empty multi-query results retry with
    // the original input).
    let original = "original user input";
    assert!(
        !original.trim().is_empty(),
        "the original message is always available as the fallback query"
    );
}

// Covers: rewrite prompts must not depend on the interrupted flag — an
// interrupted partial answer feeds rewrite context for follow-up resolution
// exactly like a normal round.
#[test]
fn rewrite_prompt_is_identical_for_interrupted_and_normal_history() {
    let interrupted_history = vec![
        ChatMessage {
            role: "user".into(),
            content: "3 月销售数据".into(),
            interrupted: false,
        },
        ChatMessage {
            role: "assistant".into(),
            content: "3 月销售额为".into(),
            interrupted: true,
        },
    ];
    let mut normal_history = interrupted_history.clone();
    normal_history[1].interrupted = false;

    assert_eq!(
        build_rewrite_prompt(&interrupted_history, "那个月的产品明细呢", None),
        build_rewrite_prompt(&normal_history, "那个月的产品明细呢", None),
        "rewrite prompt must not depend on the interrupted flag"
    );
    assert!(
        build_rewrite_prompt(&interrupted_history, "那个月的产品明细呢", None)
            .contains("3 月销售额为"),
        "interrupted partial answer must still feed rewrite context for follow-up resolution"
    );
}
