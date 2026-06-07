//! Scenario tests for GET /api/chat/suggestions — match_locale pure function.
//!
//! Verifies the locale matching logic that powers the suggestions endpoint:
//! exact match -> longest prefix match -> "default" key -> empty vec.
//! All tests call `match_locale` directly with constructed config values.
//! No AppState construction needed.

use std::collections::HashMap;

use super::chat::match_locale;

// ---------------------------------------------------------------------------
// Scenario 1: Default group, no locale param
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 1; no locale provided -> default key lookup path.

#[test]
fn default_group_no_locale_returns_default_questions() {
    let config = Some(HashMap::from([
        (
            "default".to_string(),
            vec!["What is Rust?".to_string(), "How to install?".to_string()],
        ),
        ("en".to_string(), vec!["English question".to_string()]),
    ]));
    let result = match_locale(&config, None);

    assert_eq!(
        result,
        vec!["What is Rust?", "How to install?"],
        "without locale param, must return the 'default' group questions"
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Exact locale match (case-insensitive)
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 2; exact match path.

#[test]
fn exact_locale_match_returns_matching_questions() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        (
            "zh-CN".to_string(),
            vec!["如何开始?".to_string(), "有什么功能?".to_string()],
        ),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["如何开始?", "有什么功能?"],
        "exact locale match must return the matching group's questions"
    );
}

// User Story: US-CORE-028
// Covers: Exact match is case-insensitive; locale "zh-cn" matches key "zh-CN".

#[test]
fn exact_locale_match_is_case_insensitive() {
    let config = Some(HashMap::from([(
        "zh-CN".to_string(),
        vec!["Chinese question".to_string()],
    )]));
    let result = match_locale(&config, Some("zh-cn"));

    assert_eq!(
        result,
        vec!["Chinese question"],
        "exact match must be case-insensitive: 'zh-cn' matches key 'zh-CN'"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3a: No prefix match, falls to default
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 3a; no prefix match for locale -> default path.

#[test]
fn no_prefix_match_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
        ("zh-CN".to_string(), vec!["CN Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("ja"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'ja' has no exact or prefix match, must fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3b: Prefix match to shorter key
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 3b; longest prefix match path.

#[test]
fn prefix_match_to_shorter_key() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
        ("zh-CN".to_string(), vec!["CN Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-TW"));

    assert_eq!(
        result,
        vec!["Chinese Q"],
        "'zh-TW' has no exact match; 'zh' is a prefix -> returns 'zh' group questions"
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: No suggested_questions configured
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 4; None config handling.

#[test]
fn no_config_returns_empty_array() {
    let result = match_locale(&None, Some("en"));

    assert!(
        result.is_empty(),
        "None config must return empty vec, not error"
    );
}

// User Story: US-CORE-028
// Covers: Empty HashMap is treated like None.

#[test]
fn empty_hashmap_returns_empty_array() {
    let config = Some(HashMap::new());
    let result = match_locale(&config, Some("en"));

    assert!(result.is_empty(), "empty HashMap must return empty vec");
}

// ---------------------------------------------------------------------------
// Scenario 5: Truncation at 10
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design 5.1 scenario 5; truncation to MAX_SUGGESTIONS (10).

#[test]
fn more_than_ten_questions_truncated_to_ten() {
    let questions: Vec<String> = (1..=12).map(|i| format!("Question {i}")).collect();
    let config = Some(HashMap::from([("default".to_string(), questions)]));
    let result = match_locale(&config, None);

    assert_eq!(
        result.len(),
        10,
        "must truncate to 10 questions when config has more"
    );
    assert_eq!(result[0], "Question 1", "first question must be preserved");
    assert_eq!(result[9], "Question 10", "10th question must be preserved");
    assert!(
        !result.contains(&"Question 11".to_string()),
        "11th question must be truncated"
    );
    assert!(
        !result.contains(&"Question 12".to_string()),
        "12th question must be truncated"
    );
}

// ---------------------------------------------------------------------------
// Scenario 6: Empty locale string
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Locale validation; empty string fails format check, falls to default.

#[test]
fn empty_locale_string_treated_as_no_locale() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some(""));

    assert_eq!(
        result,
        vec!["Default Q"],
        "empty string locale fails validation, must fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 7: Invalid locale format
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Locale validation: digits in locale string rejected -> default fallback.

#[test]
fn invalid_locale_with_digits_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("abc123"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'abc123' contains digits, must be rejected and fall back to default"
    );
}

// User Story: US-CORE-028
// Covers: Locale validation: overly long locale string rejected -> default fallback.

#[test]
fn invalid_locale_too_long_falls_to_default() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("en".to_string(), vec!["English Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("en-US-extra-long"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'en-US-extra-long' exceeds max locale length, must be rejected and fall back to default"
    );
}

// User Story: US-CORE-028
// Covers: Locale validation: underscore separator rejected -> default fallback.

#[test]
fn invalid_locale_with_underscore_falls_to_default() {
    let config = Some(HashMap::from([(
        "default".to_string(),
        vec!["Default Q".to_string()],
    )]));
    let result = match_locale(&config, Some("en_US"));

    assert_eq!(
        result,
        vec!["Default Q"],
        "'en_US' uses underscore instead of hyphen, must be rejected and fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 8: Exact match but empty questions array
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Exact match returns whatever the config says, even if empty.

#[test]
fn exact_match_with_empty_array_returns_empty() {
    let config = Some(HashMap::from([
        ("default".to_string(), vec!["Default Q".to_string()]),
        ("fr".to_string(), vec![]),
    ]));
    let result = match_locale(&config, Some("fr"));

    assert!(
        result.is_empty(),
        "exact match to 'fr' must return its empty array, not fall back to default"
    );
}

// ---------------------------------------------------------------------------
// Scenario 9: Multiple prefix matches, longest wins
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Design BE-D01 spec "longest prefix match" -- most specific key wins.

#[test]
fn multiple_prefix_matches_longest_wins() {
    let config = Some(HashMap::from([
        ("z".to_string(), vec!["Z Q".to_string()]),
        ("zh".to_string(), vec!["ZH Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["ZH Q"],
        "'zh-CN' matches prefixes 'z' and 'zh'; 'zh' is longest, must win"
    );
}

// User Story: US-CORE-028
// Covers: Multiple prefix matches including 3-letter key; longest still wins.

#[test]
fn multiple_prefix_matches_with_three_char_key_longest_wins() {
    let config = Some(HashMap::from([
        ("z".to_string(), vec!["Z Q".to_string()]),
        ("zh".to_string(), vec!["ZH Q".to_string()]),
        ("zh-".to_string(), vec!["ZH-HYPHEN Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-CN"));

    assert_eq!(
        result,
        vec!["ZH Q"],
        "'zh-CN' matches 'z', 'zh', but not 'zh-' (hyphen is not a prefix match since locale \
         validation rejects keys with trailing hyphen); 'zh' is longest valid prefix"
    );
}

// ---------------------------------------------------------------------------
// No match and no default key
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: When no locale match exists and no "default" key is configured,
//         the function returns an empty vec rather than panicking.

#[test]
fn no_match_and_no_default_key_returns_empty() {
    let config = Some(HashMap::from([
        ("en".to_string(), vec!["English Q".to_string()]),
        ("zh".to_string(), vec!["Chinese Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("ja"));

    assert!(
        result.is_empty(),
        "no match for 'ja' and no 'default' key must return empty vec"
    );
}

// User Story: US-CORE-028
// Covers: None locale without default key returns empty vec.

#[test]
fn none_locale_without_default_returns_empty() {
    let config = Some(HashMap::from([(
        "en".to_string(),
        vec!["English Q".to_string()],
    )]));
    let result = match_locale(&config, None);

    assert!(
        result.is_empty(),
        "None locale without 'default' key must return empty vec"
    );
}

// ---------------------------------------------------------------------------
// Case-insensitive prefix matching
// ---------------------------------------------------------------------------

// User Story: US-CORE-028
// Covers: Prefix matching is case-insensitive on both locale and key.

#[test]
fn prefix_match_is_case_insensitive() {
    let config = Some(HashMap::from([
        ("ZH".to_string(), vec!["Chinese Q".to_string()]),
        ("default".to_string(), vec!["Default Q".to_string()]),
    ]));
    let result = match_locale(&config, Some("zh-tw"));

    assert_eq!(
        result,
        vec!["Chinese Q"],
        "prefix match must be case-insensitive: 'zh-tw' matches key 'ZH'"
    );
}
