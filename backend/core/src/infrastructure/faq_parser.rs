//! FAQ JSONL file parser.
//!
//! Parses a FAQ JSONL file (one JSON object per line:
//! `{question, answer, tags?, locale?}` per line) into `ParsedChunk` objects,
//! one per Q&A pair. Each chunk contains a Markdown representation of the
//! question (as H2) plus the answer for embedding and search.

use serde::Deserialize;
use serde_json::Value;
use std::fmt;
use uuid::Uuid;

use super::xlsx_parser::{ContentType, ParsedChunk};

// ---------------------------------------------------------------------------
// Internal deserialization types
// ---------------------------------------------------------------------------

/// Tag value accepts either a comma-separated string or a string array.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TagsValue {
    String(String),
    Array(Vec<String>),
}

impl TagsValue {
    /// Convert to a `Vec<String>`. For the `String` variant, split by `,`,
    /// trim whitespace, and drop empty entries. For the `Array` variant,
    /// return as-is.
    fn into_vec(self) -> Vec<String> {
        match self {
            TagsValue::String(s) => s
                .split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect(),
            TagsValue::Array(items) => items,
        }
    }
}

/// One FAQ item. `tags` and `locale` are optional; unknown fields (e.g.
/// `category`) are silently ignored by `serde` default behavior.
#[derive(Debug, Deserialize)]
struct FaqItem {
    question: String,
    answer: String,
    #[serde(default)]
    tags: Option<TagsValue>,
    #[serde(default)]
    locale: Option<String>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error type for FAQ JSONL file parsing.
#[derive(Debug)]
pub enum FaqParseError {
    InvalidEncoding,
    InvalidJson { line: usize, detail: String },
    EmptyFile,
    MissingRequiredFields { index: usize, fields: Vec<String> },
    EmptyField { index: usize, field: String },
}

impl fmt::Display for FaqParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FaqParseError::InvalidEncoding => write!(f, "文件编码不支持，仅支持 UTF-8"),
            FaqParseError::InvalidJson { line, detail } => {
                write!(f, "第 {line} 行 JSON 格式无效: {detail}")
            }
            FaqParseError::EmptyFile => write!(f, "文件中没有可用的问答数据"),
            FaqParseError::MissingRequiredFields { index, fields } => {
                // Hand-render fields as slash-joined (NOT Debug format) so the
                // output matches the design doc §4.2.2 table:
                //   "第 0 条问答缺少必填字段: question/answer"
                write!(f, "第 {index} 条问答缺少必填字段: {}", fields.join("/"))
            }
            FaqParseError::EmptyField { index, field } => {
                write!(f, "第 {index} 条问答的 {field} 不能为空")
            }
        }
    }
}

impl std::error::Error for FaqParseError {}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse a FAQ JSONL file into one `ParsedChunk` per Q&A pair.
///
/// Processing:
/// 1. UTF-8 decode (BOM-stripped afterward)
/// 2. Split by lines; skip blank lines (after trim)
/// 3. Each non-blank line is parsed as a JSON object; non-objects or
///    syntax errors surface as `InvalidJson { line, .. }` where `line`
///    is the physical line number (1-based).
/// 4. Required-field presence / non-empty checks use the 0-based index
///    of the non-blank entry (blank lines do not increment the index).
/// 5. Empty file (no non-blank line) → `EmptyFile`.
///
/// Each FAQ produces a `ParsedChunk` with:
/// - `content`: `"## {question}\n\n{answer}"` (Markdown H2 heading + answer)
/// - `title`: the question text
/// - `page_id`: fresh UUID v7 (each Q&A is its own knowledge page)
/// - `tags`: parsed from optional `tags` field (string or array)
/// - `locale`: from optional `locale` field
/// - `content_type`: `ContentType::None` (use default jieba tokenization)
/// - `fts_tokens`: `None` (no format-specific pre-tokenization)
pub fn parse_faq_file(bytes: &[u8]) -> Result<Vec<ParsedChunk>, FaqParseError> {
    // UTF-8 decode
    let utf8_str = std::str::from_utf8(bytes).map_err(|_| FaqParseError::InvalidEncoding)?;

    // Strip a leading UTF-8 BOM if present
    let stripped = utf8_str.strip_prefix('\u{FEFF}').unwrap_or(utf8_str);

    let mut chunks: Vec<ParsedChunk> = Vec::new();
    let mut index: usize = 0; // 0-based index over non-blank entries
    let mut saw_non_blank = false;

    for (line_no, raw_line) in stripped.lines().enumerate() {
        // line_no is 0-based here; physical line number is line_no + 1
        let physical_line = line_no + 1;

        // Skip blank lines (after trim); they do not increment `index`
        if raw_line.trim().is_empty() {
            continue;
        }
        saw_non_blank = true;

        // Parse line as JSON value
        let value: Value =
            serde_json::from_str(raw_line).map_err(|e| FaqParseError::InvalidJson {
                line: physical_line,
                detail: e.to_string(),
            })?;

        // Root must be a JSON object
        if !value.is_object() {
            return Err(FaqParseError::InvalidJson {
                line: physical_line,
                detail: "expected JSON object".to_string(),
            });
        }

        // Manual pre-validation: required fields question and answer must
        // both be present. We check field presence (not type) here so that
        // the MissingRequiredFields.fields vector has deterministic content
        // in a stable order (question first, then answer).
        let mut missing: Vec<String> = Vec::new();
        if value.get("question").is_none() {
            missing.push("question".to_string());
        }
        if value.get("answer").is_none() {
            missing.push("answer".to_string());
        }
        if !missing.is_empty() {
            return Err(FaqParseError::MissingRequiredFields {
                index,
                fields: missing,
            });
        }

        // Deserialize via serde (catches type mismatches etc.)
        let item: FaqItem =
            serde_json::from_value(value).map_err(|e| FaqParseError::InvalidJson {
                line: physical_line,
                detail: e.to_string(),
            })?;

        // Empty-string validation (after trim)
        if item.question.trim().is_empty() {
            return Err(FaqParseError::EmptyField {
                index,
                field: "question".to_string(),
            });
        }
        if item.answer.trim().is_empty() {
            return Err(FaqParseError::EmptyField {
                index,
                field: "answer".to_string(),
            });
        }

        let tags = item.tags.map(|t| t.into_vec()).unwrap_or_default();

        // Explicit 11-field initialization (no `..Default::default()`) to
        // keep the ParsedChunk contract stable against future schema drift.
        chunks.push(ParsedChunk {
            content: format!("## {}\n\n{}", item.question, item.answer),
            page_id: Uuid::now_v7(),
            sub_index: None,
            title: item.question,
            locale: item.locale,
            link: None,
            tags,
            section: None,
            chunk_count: None,
            content_type: ContentType::None,
            fts_tokens: None,
        });

        index += 1;
    }

    if !saw_non_blank {
        return Err(FaqParseError::EmptyFile);
    }

    Ok(chunks)
}
