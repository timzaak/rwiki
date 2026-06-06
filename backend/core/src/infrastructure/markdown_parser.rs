use uuid::Uuid;

use super::xlsx_parser::{ContentType, ParsedChunk};

/// Error type for markdown/mdx file parsing.
#[derive(Debug, thiserror::Error)]
pub enum MarkdownParseError {
    #[error("文件内容为空")]
    EmptyFile,
    #[error("文件编码不支持，仅支持 UTF-8")]
    InvalidEncoding,
    #[error("frontmatter 格式错误: 未闭合")]
    FrontmatterNotClosed,
    #[error("frontmatter 格式错误: 第 {line_number} 行 '{line}'")]
    InvalidFrontmatterLine { line_number: usize, line: String },
    #[error("frontmatter 格式错误: 字段 '{field}' 重复")]
    DuplicateField { field: String },
}

/// Parse .md / .mdx file bytes into a ParsedChunk.
///
/// - bytes: raw file bytes
/// - file_name: original file name (used for title fallback)
///
/// Returns a single ParsedChunk with a unique page_id, or a MarkdownParseError.
pub fn parse_markdown_file(
    bytes: &[u8],
    file_name: &str,
) -> Result<ParsedChunk, MarkdownParseError> {
    // UTF-8 decode
    let mut text = std::str::from_utf8(bytes)
        .map_err(|_| MarkdownParseError::InvalidEncoding)?
        .to_string();

    // Strip BOM if present
    if let Some(stripped) = text.strip_prefix('\u{FEFF}') {
        text = stripped.to_string();
    }

    // Detect and extract frontmatter
    let (frontmatter, body) = extract_frontmatter(&text)?;

    // Trim body; if empty after trimming -> EmptyFile
    let body = body.trim();
    if body.is_empty() {
        return Err(MarkdownParseError::EmptyFile);
    }

    // Parse frontmatter fields
    let fields = parse_frontmatter_fields(frontmatter)?;

    // Extract known fields
    let fm_title = fields.get("title").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let fm_locale = fields.get("locale").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let fm_link = fields.get("link").and_then(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    let fm_tags: Vec<String> = fields
        .get("tags")
        .map(|v| {
            v.split(',')
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    // Title fallback: frontmatter > first # heading > filename without extension
    let title = resolve_title(fm_title.as_deref(), body, file_name);

    Ok(ParsedChunk {
        content: body.to_string(),
        page_id: Uuid::now_v7(),
        sub_index: None,
        title,
        locale: fm_locale,
        link: fm_link,
        tags: fm_tags,
        section: None,
        chunk_count: None,
        content_type: ContentType::None,
        fts_tokens: None,
    })
}

/// Extract frontmatter and body from text.
/// Returns (frontmatter_lines, body_text).
fn extract_frontmatter(text: &str) -> Result<(Vec<&str>, &str), MarkdownParseError> {
    let lines: Vec<&str> = text.lines().collect();

    // First line must be standalone "---" to trigger frontmatter detection
    if lines.is_empty() || lines[0].trim() != "---" {
        // No frontmatter; body = full text
        return Ok((Vec::new(), text));
    }

    // Find closing "---"
    // Compute byte offset by iterating through lines
    let mut byte_offset = 0;
    // Skip past the opening "---" line
    byte_offset += lines[0].len();
    // Skip the newline after opening ---
    if text.as_bytes().get(byte_offset) == Some(&b'\r') {
        byte_offset += 1;
    }
    if text.as_bytes().get(byte_offset) == Some(&b'\n') {
        byte_offset += 1;
    }

    for i in 1..lines.len() {
        if lines[i].trim() == "---" {
            let frontmatter = lines[1..i].to_vec();
            // Body starts after the closing "---" line
            let body_start = byte_offset + lines[i].len();
            let body = if body_start < text.len() {
                // Skip newline after closing ---
                let mut start = body_start;
                if text.as_bytes().get(start) == Some(&b'\r') {
                    start += 1;
                }
                if text.as_bytes().get(start) == Some(&b'\n') {
                    start += 1;
                }
                &text[start..]
            } else {
                ""
            };
            return Ok((frontmatter, body));
        }
        // Advance past this line
        byte_offset += lines[i].len();
        // Skip newline
        if text.as_bytes().get(byte_offset) == Some(&b'\r') {
            byte_offset += 1;
        }
        if text.as_bytes().get(byte_offset) == Some(&b'\n') {
            byte_offset += 1;
        }
    }

    // Opening --- found but no closing ---
    Err(MarkdownParseError::FrontmatterNotClosed)
}

/// Parse frontmatter lines into key-value pairs.
/// Only recognized fields: title, locale, link, tags.
/// Returns an error on duplicate fields or invalid lines.
fn parse_frontmatter_fields(
    lines: Vec<&str>,
) -> Result<std::collections::HashMap<String, String>, MarkdownParseError> {
    let mut fields = std::collections::HashMap::new();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Must contain at least one ':'
        let colon_pos = trimmed.find(':');
        match colon_pos {
            None => {
                return Err(MarkdownParseError::InvalidFrontmatterLine {
                    line_number: idx + 2, // 1-based: line 1 is opening ---, so frontmatter lines start at 2
                    line: trimmed.to_string(),
                });
            }
            Some(pos) => {
                let key = trimmed[..pos].trim();
                let value = trimmed[pos + 1..].trim();

                // Only recognize known fields
                if !matches!(key, "title" | "locale" | "link" | "tags") {
                    continue; // unknown fields ignored
                }

                // Check for duplicate
                if fields.contains_key(key) {
                    return Err(MarkdownParseError::DuplicateField {
                        field: key.to_string(),
                    });
                }

                fields.insert(key.to_string(), value.to_string());
            }
        }
    }

    Ok(fields)
}

/// Resolve title using fallback chain:
/// 1. frontmatter title (if non-empty)
/// 2. First "# heading" in body
/// 3. Filename with .md/.mdx extension stripped
fn resolve_title(fm_title: Option<&str>, body: &str, file_name: &str) -> String {
    // 1. frontmatter title
    if let Some(t) = fm_title {
        if !t.is_empty() {
            return t.to_string();
        }
    }

    // 2. First # heading in body
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let heading = rest.trim_start_matches('#').trim();
            if !heading.is_empty() {
                return heading.to_string();
            }
        }
    }

    // 3. Filename without extension
    let name = file_name
        .strip_suffix(".mdx")
        .or_else(|| file_name.strip_suffix(".md"))
        .unwrap_or(file_name);
    name.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_title_prefers_frontmatter() {
        let result = resolve_title(Some("FM Title"), "# Heading", "file.md");
        assert_eq!(result, "FM Title");
    }

    #[test]
    fn resolve_title_falls_back_to_h1() {
        let result = resolve_title(None, "# Heading\nBody", "file.md");
        assert_eq!(result, "Heading");
    }

    #[test]
    fn resolve_title_falls_back_to_filename() {
        let result = resolve_title(None, "No heading here", "my-doc.md");
        assert_eq!(result, "my-doc");
    }

    #[test]
    fn resolve_title_strips_mdx() {
        let result = resolve_title(None, "No heading", "doc.mdx");
        assert_eq!(result, "doc");
    }
}
