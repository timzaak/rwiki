//! Scenario tests for FAQ JSONL parsing.
//!
//! Covers the FAQ parser contract documented in design
//! `faq_format_support.md` §6.1: each line is an independent JSON object;
//! blank lines are skipped; required-field and value semantics are pinned
//! down per entry.

use super::faq_parser::{parse_faq_file, FaqParseError};
use super::xlsx_parser::ContentType;

// ---------------------------------------------------------------------------
// 1. Valid JSONL -> chunks
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a FAQ JSONL file with
// multiple Q&A pairs to be parsed into one chunk per pair, each formatted
// as a Markdown H2 heading (the question) followed by the answer, so that
// both the question and answer participate in embedding and section
// tracking.
// Covers: US-CORE-032, BE-D01

#[test]
fn valid_faq_jsonl_produces_chunks() {
    let jsonl =
        "{\"question\": \"Q1\", \"answer\": \"A1\"}\n{\"question\": \"Q2\", \"answer\": \"A2\"}\n";
    let chunks = parse_faq_file(jsonl.as_bytes()).expect("valid FAQ JSONL should parse");

    assert_eq!(chunks.len(), 2, "one chunk per Q&A pair");
    for (i, chunk) in chunks.iter().enumerate() {
        let n = i + 1;
        assert!(
            chunk.content.starts_with("## "),
            "chunk {} content must start with '## ' (Markdown H2), got: {}",
            n,
            chunk.content
        );
        assert!(
            chunk.content.contains(&format!("Q{}", n)),
            "chunk {} content must contain the question, got: {}",
            n,
            chunk.content
        );
        assert_eq!(
            chunk.title,
            format!("Q{}", n),
            "chunk {} title must equal the question text",
            n
        );
        assert_eq!(
            chunk.content_type,
            ContentType::None,
            "FAQ chunks use default jieba tokenization (ContentType::None)"
        );
        assert!(
            chunk.fts_tokens.is_none(),
            "FAQ chunks have no pre-computed FTS tokens"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. tags as comma-separated string
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to supply `tags` as a single
// comma-separated string so I can annotate FAQ pages without authoring a JSON
// array per entry.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_parses_tags_string_value() {
    let jsonl = "{\"question\": \"Q\", \"answer\": \"A\", \"tags\": \"t1,t2\"}\n";
    let chunks = parse_faq_file(jsonl.as_bytes()).expect("tags string should parse");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].tags, vec!["t1".to_string(), "t2".to_string()]);
}

// ---------------------------------------------------------------------------
// 3. tags as string array
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to supply `tags` as a JSON
// string array so that tags with embedded commas are preserved exactly.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_parses_tags_array_value() {
    let jsonl = "{\"question\": \"Q\", \"answer\": \"A\", \"tags\": [\"t1\", \"t2\"]}\n";
    let chunks = parse_faq_file(jsonl.as_bytes()).expect("tags array should parse");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].tags, vec!["t1".to_string(), "t2".to_string()]);
}

// ---------------------------------------------------------------------------
// 4. optional locale
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to attach a `locale` to a FAQ
// entry so that the FAQ page is searchable by language.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_parses_optional_locale() {
    let jsonl = "{\"question\": \"Q\", \"answer\": \"A\", \"locale\": \"zh\"}\n";
    let chunks = parse_faq_file(jsonl.as_bytes()).expect("locale should parse");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].locale.as_deref(), Some("zh"));
}

// ---------------------------------------------------------------------------
// 5. category and unknown fields ignored
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to keep `category` and other
// extra keys in my FAQ source files without breaking uploads, so the parser
// only enforces the contract on `question`/`answer` and tolerates the rest.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_ignores_category_and_unknown_fields() {
    let jsonl = "{\"question\": \"Q\", \"answer\": \"A\", \"category\": \"cat\", \"extra\": 42, \"nested\": {\"x\": 1}}\n";
    let chunks = parse_faq_file(jsonl.as_bytes()).expect("unknown fields should not block parsing");

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].title, "Q");
    assert!(chunks[0].content.contains("A"));
}

// ---------------------------------------------------------------------------
// 6. missing question
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the parser to reject FAQ
// entries that are missing `question` with a precise pointer to which entry
// and which field, so I can fix the source file at the exact location.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_missing_question_returns_error() {
    let jsonl = "{\"answer\": \"A\"}\n";
    let result = parse_faq_file(jsonl.as_bytes());

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::MissingRequiredFields { index, fields } => {
            assert_eq!(index, 0, "first entry");
            assert!(
                fields.iter().any(|f| f == "question"),
                "fields must mention 'question', got: {:?}",
                fields
            );
        }
        other => panic!("expected MissingRequiredFields, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 7. missing answer
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the parser to reject FAQ
// entries that are missing `answer` with a precise pointer to which entry
// and which field, so I can fix the source file at the exact location.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_missing_answer_returns_error() {
    let jsonl = "{\"question\": \"Q\"}\n";
    let result = parse_faq_file(jsonl.as_bytes());

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::MissingRequiredFields { index, fields } => {
            assert_eq!(index, 0, "first entry");
            assert!(
                fields.iter().any(|f| f == "answer"),
                "fields must mention 'answer', got: {:?}",
                fields
            );
        }
        other => panic!("expected MissingRequiredFields, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 8. empty question
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want whitespace-only `question`
// values to be rejected so that empty FAQ entries cannot silently enter the
// knowledge base and degrade search quality.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_empty_question_returns_error() {
    let jsonl = "{\"question\": \"\", \"answer\": \"A\"}\n";
    let result = parse_faq_file(jsonl.as_bytes());

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::EmptyField { index, field } => {
            assert_eq!(index, 0, "first entry");
            assert_eq!(field, "question");
        }
        other => panic!("expected EmptyField(question), got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 9. empty answer
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want whitespace-only `answer`
// values to be rejected so that empty FAQ entries cannot silently enter the
// knowledge base and degrade search quality.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_empty_answer_returns_error() {
    let jsonl = "{\"question\": \"Q\", \"answer\": \"\"}\n";
    let result = parse_faq_file(jsonl.as_bytes());

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::EmptyField { index, field } => {
            assert_eq!(index, 0, "first entry");
            assert_eq!(field, "answer");
        }
        other => panic!("expected EmptyField(answer), got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 10. empty file (no non-blank line)
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want an empty FAQ file (or one
// with only blank lines) to be rejected with a clear "no usable Q&A data"
// message so that I notice the file is empty instead of creating an empty
// draft document.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_empty_file_returns_error() {
    let result = parse_faq_file(b"");

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::EmptyFile => {}
        other => panic!("expected EmptyFile, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 11. file with only blank lines -> EmptyFile
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a file that contains only
// blank lines to be treated as an empty FAQ file, not as a valid one, so I
// don't silently create an empty knowledge base document.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_only_blank_lines_returns_empty_file() {
    let result = parse_faq_file(b"   \n\t\n   \n");

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::EmptyFile => {}
        other => panic!("expected EmptyFile, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 12. single line invalid JSON -> InvalidJson with physical line number
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want malformed JSON on a single
// line to be rejected with a clear "invalid JSON" message including the
// physical line number, so I can locate the syntax error in the source file.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_invalid_json_single_line_returns_error() {
    let jsonl = b"not json";
    let result = parse_faq_file(jsonl);

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::InvalidJson { line, .. } => {
            assert_eq!(line, 1, "first physical line");
        }
        other => panic!("expected InvalidJson, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 13. line not a JSON object -> InvalidJson "expected JSON object"
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want a line that parses as a JSON
// value other than an object (string, number, array) to be rejected as an
// invalid FAQ entry, so only Q&A objects can populate the knowledge base.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_non_object_line_returns_error() {
    let cases: &[&[u8]] = &[b"\"just a string\"\n", b"42\n", b"[1, 2]\n"];
    for jsonl in cases {
        let result = parse_faq_file(jsonl);
        assert!(
            result.is_err(),
            "expected error for: {}",
            String::from_utf8_lossy(jsonl)
        );
        match result.unwrap_err() {
            FaqParseError::InvalidJson { line, detail } => {
                assert_eq!(line, 1, "first physical line");
                assert!(
                    detail.contains("expected JSON object"),
                    "detail must say 'expected JSON object', got: {detail}"
                );
            }
            other => panic!("expected InvalidJson, got: {:?}", other),
        }
    }
}

// ---------------------------------------------------------------------------
// 14. physical line number correctly reported when first line is blank
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the line number in JSON
// syntax errors to reflect the physical position in the file (1-based,
// counting blank lines), so I can jump to the exact row in my editor.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_invalid_json_reports_physical_line_number() {
    // Line 1 is blank, line 2 has bad JSON
    let jsonl = b"\nnot json\n";
    let result = parse_faq_file(jsonl);

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::InvalidJson { line, .. } => {
            assert_eq!(line, 2, "error must reference physical line 2");
        }
        other => panic!("expected InvalidJson, got: {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// 15. UTF-8 BOM stripped before parsing
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want my editor's UTF-8 BOM (which
// some Windows tools prepend) to be tolerated so my FAQ JSONL file still
// parses correctly.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_strips_utf8_bom() {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice("\u{FEFF}".as_bytes()); // UTF-8 BOM
    bytes.extend_from_slice(b"{\"question\": \"Q\", \"answer\": \"A\"}\n");
    let chunks = parse_faq_file(&bytes).expect("BOM-prefixed file should parse");
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].title, "Q");
}

// ---------------------------------------------------------------------------
// 16. blank lines between entries are skipped
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want to be able to leave blank
// (or whitespace-only) lines between Q&A entries for readability, without
// breaking the parser.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_skips_blank_lines_between_entries() {
    // blank line, real entry, pure-space line, tab-only line, real entry
    let jsonl = b"\n{\"question\": \"Q1\", \"answer\": \"A1\"}\n   \n\t\n{\"question\": \"Q2\", \"answer\": \"A2\"}\n";
    let chunks = parse_faq_file(jsonl).expect("blank lines should be skipped");

    assert_eq!(
        chunks.len(),
        2,
        "two non-blank entries must produce 2 chunks"
    );
    assert_eq!(chunks[0].title, "Q1");
    assert_eq!(chunks[1].title, "Q2");
}

// ---------------------------------------------------------------------------
// 17. CRLF line endings tolerated
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor on Windows, I want my CRLF-terminated
// FAQ file to parse correctly, so I don't have to convert line endings before
// upload.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_tolerates_crlf_line_endings() {
    let jsonl = b"{\"question\": \"Q1\", \"answer\": \"A1\"}\r\n{\"question\": \"Q2\", \"answer\": \"A2\"}\r\n";
    let chunks = parse_faq_file(jsonl).expect("CRLF should be tolerated");

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].title, "Q1");
    assert_eq!(chunks[1].title, "Q2");
}

// ---------------------------------------------------------------------------
// 18. index numbering skips blank lines
// ---------------------------------------------------------------------------

// User Story: As a knowledge base editor, I want the `index` field in
// `MissingRequiredFields` to count only non-blank entries, so the error
// points to the right Q&A even when the file has blank separator lines.
// Covers: US-CORE-032, BE-D01

#[test]
fn faq_missing_field_index_skips_blank_lines() {
    // blank, valid entry (index 0), blank, missing-answer entry (index 1)
    let jsonl = b"\n{\"question\": \"Q1\", \"answer\": \"A1\"}\n\n{\"question\": \"Q2\"}\n";
    let result = parse_faq_file(jsonl);

    assert!(result.is_err());
    match result.unwrap_err() {
        FaqParseError::MissingRequiredFields { index, fields } => {
            assert_eq!(index, 1, "second non-blank entry must have index 1");
            assert!(fields.iter().any(|f| f == "answer"));
        }
        other => panic!("expected MissingRequiredFields, got: {:?}", other),
    }
}
