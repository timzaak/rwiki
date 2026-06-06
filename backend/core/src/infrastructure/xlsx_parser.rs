use calamine::{open_workbook_from_rs, Data, Range, Reader, Xlsx};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use uuid::Uuid;

/// Document content type for tokenizer strategy routing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    /// Default: use generic jieba tokenization
    #[default]
    None,
    /// OpenAPI 3.x document: use format-aware tokenization
    OpenApi,
}

/// Represents one row as a text chunk with header context.
#[derive(Debug, Clone, Default)]
pub struct ParsedChunk {
    /// Formatted text: "header1: value1, header2: value2, ..."
    pub content: String,
    /// Knowledge page-level ID (UUID v7), uniquely identifies a page across xlsx rows and markdown files.
    pub page_id: Uuid,
    /// Sub-index for chunks split from a single row (None for original chunks)
    pub sub_index: Option<usize>,
    // Wiki metadata
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    pub tags: Vec<String>,
    pub section: Option<String>,
    /// Number of chunks the original row was split into (None for original/unsplit chunks)
    pub chunk_count: Option<usize>,
    /// Document content type for tokenizer strategy routing
    pub content_type: ContentType,
    /// Pre-computed FTS tokenization result; OpenAPI docs fill this at parse time, others are None
    pub fts_tokens: Option<String>,
}

/// Structured Wiki page parsed from a single xlsx row.
#[derive(Debug, Clone)]
pub struct WikiPage {
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    pub tags: Vec<String>,
    pub markdown: String,
    pub page_id: Uuid,
}

/// Validation error for a specific row.
#[derive(Debug, Clone)]
pub struct RowValidationError {
    /// 1-based Excel row number (header is row 1, first data row is row 2).
    pub excel_row: usize,
    pub missing_fields: Vec<String>,
}

/// Result of structured xlsx parsing.
#[derive(Debug)]
pub struct ParseWikiResult {
    pub pages: Vec<WikiPage>,
}

/// Error type for wiki xlsx parsing.
#[derive(Debug)]
pub enum WikiParseError {
    MissingRequiredColumns { missing: Vec<String> },
    RowValidationFailed { errors: Vec<RowValidationError> },
    NoDataRows,
    InvalidFormat(String),
}

impl std::fmt::Display for WikiParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WikiParseError::MissingRequiredColumns { missing } => {
                write!(f, "缺少必填列: {}", missing.join("、"))
            }
            WikiParseError::RowValidationFailed { errors } => {
                let details: Vec<String> = errors
                    .iter()
                    .map(|e| format!("第 {} 行缺少 {}", e.excel_row, e.missing_fields.join("、")))
                    .collect();
                write!(f, "以下行数据不完整:\n{}", details.join("\n"))
            }
            WikiParseError::NoDataRows => write!(f, "文件中没有可用的数据行"),
            WikiParseError::InvalidFormat(msg) => write!(f, "文件格式无效: {msg}"),
        }
    }
}

impl std::error::Error for WikiParseError {}

/// Column name constants for Wiki-style xlsx parsing.
const COL_TITLE: &str = "Title";
const COL_MARKDOWN: &str = "Markdown Content";
const COL_LOCALE: &str = "Locale";
const COL_LINK: &str = "Link";
const COL_TAGS: &str = "Tags";

/// Parse xlsx bytes into structured Wiki pages.
///
/// Opens the xlsx from bytes, reads the first sheet, extracts headers from
/// the first row, validates required columns (Title, Markdown Content),
/// and maps each data row to a WikiPage.
pub fn parse_xlsx_wiki(bytes: &[u8]) -> Result<ParseWikiResult, WikiParseError> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook: Xlsx<_> = open_workbook_from_rs(cursor)
        .map_err(|e| WikiParseError::InvalidFormat(format!("无法解析 xlsx 文件: {e}")))?;

    let range: Range<Data> = workbook
        .worksheet_range_at(0)
        .ok_or_else(|| WikiParseError::InvalidFormat("xlsx 文件中没有工作表".into()))?
        .map_err(|e| WikiParseError::InvalidFormat(format!("无法读取工作表: {e}")))?;

    let mut rows = range.rows();

    // Extract headers from first row, build column name -> index mapping
    let header_row = rows
        .next()
        .ok_or_else(|| WikiParseError::InvalidFormat("xlsx 文件没有表头行".into()))?;

    let col_map: HashMap<String, usize> = header_row
        .iter()
        .enumerate()
        .filter_map(|(i, cell)| {
            let name = format_data(cell);
            if name.is_empty() {
                None
            } else {
                Some((name, i))
            }
        })
        .collect();

    if col_map.is_empty() {
        return Err(WikiParseError::InvalidFormat("表头为空".into()));
    }

    // Validate required columns
    let required = [COL_TITLE, COL_MARKDOWN];
    let missing: Vec<String> = required
        .iter()
        .filter(|col| !col_map.contains_key(**col))
        .map(|col| col.to_string())
        .collect();

    if !missing.is_empty() {
        return Err(WikiParseError::MissingRequiredColumns { missing });
    }

    let title_col = col_map[COL_TITLE];
    let markdown_col = col_map[COL_MARKDOWN];
    let locale_col = col_map.get(COL_LOCALE).copied();
    let link_col = col_map.get(COL_LINK).copied();
    let tags_col = col_map.get(COL_TAGS).copied();

    let mut pages = Vec::new();
    let mut validation_errors = Vec::new();

    for (row_index, row) in rows.enumerate() {
        let title = format_data(row.get(title_col).unwrap_or(&Data::Empty));
        let markdown = format_data(row.get(markdown_col).unwrap_or(&Data::Empty));

        // Skip completely empty rows
        if title.is_empty() && markdown.is_empty() {
            // Also check optional columns to determine if row is truly empty
            let locale_val = locale_col.map_or(String::new(), |c| {
                format_data(row.get(c).unwrap_or(&Data::Empty))
            });
            let link_val = link_col.map_or(String::new(), |c| {
                format_data(row.get(c).unwrap_or(&Data::Empty))
            });
            let tags_val = tags_col.map_or(String::new(), |c| {
                format_data(row.get(c).unwrap_or(&Data::Empty))
            });
            if locale_val.is_empty() && link_val.is_empty() && tags_val.is_empty() {
                continue;
            }
        }

        // Validate required fields
        let mut missing_fields = Vec::new();
        if title.is_empty() {
            missing_fields.push(COL_TITLE.to_string());
        }
        if markdown.is_empty() {
            missing_fields.push(COL_MARKDOWN.to_string());
        }

        if !missing_fields.is_empty() {
            validation_errors.push(RowValidationError {
                excel_row: row_index + 2, // Convert 0-based data row index to 1-based Excel row number
                missing_fields,
            });
            continue;
        }

        // Extract optional fields
        let locale = locale_col.and_then(|c| {
            let v = format_data(row.get(c).unwrap_or(&Data::Empty));
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        });

        let link = link_col.and_then(|c| {
            let v = format_data(row.get(c).unwrap_or(&Data::Empty));
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        });

        // Tags: comma-separated, trim, filter empty. Non-string types degrade to empty.
        let tags = tags_col
            .map(|c| match row.get(c).unwrap_or(&Data::Empty) {
                Data::String(s) => s
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect(),
                _ => Vec::new(),
            })
            .unwrap_or_default();

        pages.push(WikiPage {
            title,
            locale,
            link,
            tags,
            markdown,
            page_id: Uuid::now_v7(),
        });
    }

    if !validation_errors.is_empty() {
        return Err(WikiParseError::RowValidationFailed {
            errors: validation_errors,
        });
    }

    if pages.is_empty() {
        return Err(WikiParseError::NoDataRows);
    }

    Ok(ParseWikiResult { pages })
}

fn format_data(data: &Data) -> String {
    match data {
        Data::String(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("[error: {e:?}]"),
        Data::Empty => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid xlsx bytes for testing.
    /// This test verifies error handling for invalid bytes.
    #[test]
    fn parse_invalid_bytes_returns_error() {
        let result = parse_xlsx_wiki(b"not an xlsx file");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, WikiParseError::InvalidFormat(ref msg) if msg.contains("无法解析 xlsx")),
            "expected InvalidFormat with parse failure, got: {err:?}"
        );
    }
}
