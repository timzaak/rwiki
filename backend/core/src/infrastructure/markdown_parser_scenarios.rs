//! Markdown parser scenario tests — parse contracts plus
//! markdown_to_plain_text stripping scenarios for search snippets.

use super::markdown_parser::{markdown_to_plain_text, parse_markdown_file, MarkdownParseError};

/// 1. Full frontmatter .md parses successfully: title, locale, link, tags extracted, page_id non-empty.
#[test]
fn full_frontmatter_md_parses_successfully() {
    let input = "---\ntitle: My Page\nlocale: en\nlink: https://example.com\ntags: rust, ai, wiki\n---\n\n# Body content\n\nSome text here.";
    let result = parse_markdown_file(input.as_bytes(), "test.md").unwrap();

    assert_eq!(result.title, "My Page");
    assert_eq!(result.locale.as_deref(), Some("en"));
    assert_eq!(result.link.as_deref(), Some("https://example.com"));
    assert_eq!(result.tags, vec!["rust", "ai", "wiki"]);
    assert!(!result.page_id.is_nil());
    assert!(result.content.contains("Body content"));
    assert!(result.sub_index.is_none());
    assert!(result.section.is_none());
    assert!(result.chunk_count.is_none());
}

/// 2. No frontmatter: title derived from first `# heading`.
#[test]
fn no_frontmatter_title_from_h1() {
    let input = "# Hello World\n\nSome body text.";
    let result = parse_markdown_file(input.as_bytes(), "doc.md").unwrap();

    assert_eq!(result.title, "Hello World");
    assert!(result.locale.is_none());
    assert!(result.link.is_none());
    assert!(result.tags.is_empty());
    assert!(result.content.contains("Hello World"));
}

/// 3. No frontmatter, no H1: title from filename without extension.
#[test]
fn no_frontmatter_no_h1_title_from_filename() {
    let input = "Just some text without any heading.";
    let result = parse_markdown_file(input.as_bytes(), "my-document.md").unwrap();

    assert_eq!(result.title, "my-document");
}

/// 4. Partial frontmatter: missing fields use defaults.
#[test]
fn partial_frontmatter_fields() {
    let input = "---\ntitle: Only Title\n---\nBody text.";
    let result = parse_markdown_file(input.as_bytes(), "partial.md").unwrap();

    assert_eq!(result.title, "Only Title");
    assert!(result.locale.is_none());
    assert!(result.link.is_none());
    assert!(result.tags.is_empty());
}

/// 5. .mdx file uses same parser as .md, producing same structure.
#[test]
fn mdx_file_same_parser_as_md() {
    let input = "# MDX Page\n\n<Component /> here.";
    let result = parse_markdown_file(input.as_bytes(), "page.mdx").unwrap();

    assert_eq!(result.title, "MDX Page");
    assert!(result.content.contains("<Component />"));
}

/// 6. Empty file (zero bytes) returns EmptyFile error.
#[test]
fn empty_file_returns_empty_file_error() {
    let result = parse_markdown_file(b"", "empty.md");
    assert!(matches!(result, Err(MarkdownParseError::EmptyFile)));
}

/// 7. Frontmatter only, no body: returns EmptyFile.
#[test]
fn frontmatter_only_no_body_returns_empty_file() {
    let input = "---\ntitle: No Body\n---\n";
    let result = parse_markdown_file(input.as_bytes(), "nobody.md");
    assert!(matches!(result, Err(MarkdownParseError::EmptyFile)));
}

/// 8. Non-UTF-8 bytes return InvalidEncoding error.
#[test]
fn non_utf8_returns_invalid_encoding() {
    let invalid_bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x01];
    let result = parse_markdown_file(invalid_bytes, "bad.md");
    assert!(matches!(result, Err(MarkdownParseError::InvalidEncoding)));
}

/// 9. Unclosed frontmatter returns FrontmatterNotClosed error.
#[test]
fn unclosed_frontmatter_returns_not_closed() {
    let input = "---\ntitle: Unclosed\nlocale: en\n";
    let result = parse_markdown_file(input.as_bytes(), "unclosed.md");
    assert!(matches!(
        result,
        Err(MarkdownParseError::FrontmatterNotClosed)
    ));
}

/// 10. Invalid frontmatter line (no colon) returns InvalidFrontmatterLine error.
#[test]
fn invalid_frontmatter_line_returns_error() {
    let input = "---\ntitle: Valid\nbad line without colon\n---\nBody.";
    let result = parse_markdown_file(input.as_bytes(), "invalid.md");
    match result {
        Err(MarkdownParseError::InvalidFrontmatterLine { line_number, line }) => {
            assert_eq!(line_number, 3);
            assert_eq!(line, "bad line without colon");
        }
        other => panic!("expected InvalidFrontmatterLine, got: {other:?}"),
    }
}

/// 11. Duplicate field returns DuplicateField error.
#[test]
fn duplicate_field_returns_error() {
    let input = "---\ntitle: First\ntitle: Second\n---\nBody.";
    let result = parse_markdown_file(input.as_bytes(), "dup.md");
    match result {
        Err(MarkdownParseError::DuplicateField { field }) => {
            assert_eq!(field, "title");
        }
        other => panic!("expected DuplicateField, got: {other:?}"),
    }
}

/// 12. URL colon in link field does not confuse the parser.
#[test]
fn url_colon_in_link_does_not_confuse_parser() {
    let input = "---\nlink: https://example.com/path?a=1&b=2\n---\nBody text.";
    let result = parse_markdown_file(input.as_bytes(), "url.md").unwrap();

    assert_eq!(
        result.link.as_deref(),
        Some("https://example.com/path?a=1&b=2")
    );
}

/// 13. Unknown fields are ignored without error.
#[test]
fn unknown_fields_ignored() {
    let input = "---\ntitle: Hello\nauthor: Unknown\ndate: 2025-01-01\n---\nBody.";
    let result = parse_markdown_file(input.as_bytes(), "extra.md").unwrap();

    assert_eq!(result.title, "Hello");
    assert!(result.locale.is_none());
    assert!(result.link.is_none());
    assert!(result.tags.is_empty());
}

/// 14. BOM header is stripped and file parses normally.
#[test]
fn bom_header_stripped() {
    let content = "\u{FEFF}---\ntitle: BOM File\n---\nBody with BOM.";
    let result = parse_markdown_file(content.as_bytes(), "bom.md").unwrap();

    assert_eq!(result.title, "BOM File");
    assert_eq!(result.content, "Body with BOM.");
}

/// 15. Tags with commas, spaces, and empty values are correctly parsed.
#[test]
fn tags_comma_separated_with_spaces() {
    let input = "---\ntags: a, b,, c\n---\nBody.";
    let result = parse_markdown_file(input.as_bytes(), "tags.md").unwrap();

    assert_eq!(result.tags, vec!["a", "b", "c"]);
}

/// 16. Empty title in frontmatter falls back to H1 in body.
#[test]
fn empty_title_in_frontmatter_falls_back_to_h1() {
    let input = "---\ntitle:\n---\n# Heading in Body\n\nContent.";
    let result = parse_markdown_file(input.as_bytes(), "fallback.md").unwrap();

    assert_eq!(result.title, "Heading in Body");
}

/// 17. Heading/emphasis/inline-code markers disappear while their text stays readable as prose.
#[test]
fn heading_emphasis_inline_code_markers_stripped() {
    let plain = markdown_to_plain_text("# Install\n\nUse **bold** and `cargo build` here.");
    assert_eq!(plain, "Install Use bold and cargo build here.");
}

/// 18. Links keep their label but drop the URL; images keep the alt text.
#[test]
fn links_keep_label_and_images_keep_alt() {
    let plain =
        markdown_to_plain_text("See the [docs](https://example.com) and ![logo](logo.png).");
    assert_eq!(plain, "See the docs and logo.");
}

/// 19. Fenced code blocks lose the fence markers but keep the code text.
#[test]
fn fenced_code_keeps_code_text() {
    let plain = markdown_to_plain_text("Example:\n\n```rust\nfn main() {}\n```\n\nDone.");
    assert_eq!(plain, "Example: fn main() {} Done.");
}

/// 20. GFM tables flatten to space-joined cell text — no pipes or separator rows.
#[test]
fn table_flattens_to_cell_text() {
    let plain = markdown_to_plain_text("| Name | Value |\n| --- | ----- |\n| a | 1 |");
    assert_eq!(plain, "Name Value a 1");
}

/// 21. Paragraph/list boundaries collapse to one space, never gluing words together.
#[test]
fn block_boundaries_collapse_to_single_space() {
    let plain =
        markdown_to_plain_text("First paragraph.\n\nSecond paragraph.\n\n- item one\n- item two");
    assert_eq!(
        plain,
        "First paragraph. Second paragraph. item one item two"
    );
}

/// 22. Plain-text chunks (OpenAPI/FAQ content) pass through unchanged (stripping is a no-op).
#[test]
fn plain_text_passes_through_unchanged() {
    let plain = markdown_to_plain_text("Beta leading part");
    assert_eq!(plain, "Beta leading part");
}

/// 23. Inline HTML tags drop out but inner text survives; HTML-only input falls back to original.
#[test]
fn html_dropped_with_fallback_to_original() {
    let plain = markdown_to_plain_text("before <b>bold</b> after");
    assert_eq!(plain, "before bold after");

    let html_only = "<div>no markdown here</div>";
    assert_eq!(markdown_to_plain_text(html_only), html_only);
}
