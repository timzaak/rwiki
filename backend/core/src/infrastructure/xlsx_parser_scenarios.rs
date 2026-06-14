//! Scenario tests for xlsx parsing logic.
//!
//! Tests cover Wiki-style xlsx parsing: valid data, missing columns,
//! row validation, tags parsing, column order independence, and edge cases.

use super::xlsx_parser::{parse_xlsx_wiki, WikiParseError};

// ---------------------------------------------------------------------------
// Test helper: build in-memory xlsx
// ---------------------------------------------------------------------------

/// Create a minimal xlsx file in memory with the given headers and rows.
fn make_test_xlsx(headers: &[&str], rows: &[Vec<&str>]) -> Vec<u8> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();
    for (col, header) in headers.iter().enumerate() {
        worksheet
            .write_string(0, col as u16, *header)
            .expect("write header");
    }
    for (row_idx, row) in rows.iter().enumerate() {
        for (col, val) in row.iter().enumerate() {
            worksheet
                .write_string((row_idx + 1) as u32, col as u16, *val)
                .expect("write cell");
        }
    }
    workbook
        .save_to_buffer()
        .expect("save xlsx to buffer")
        .to_vec()
}

// Default Wiki-style headers
const WIKI_HEADERS: [&str; 5] = ["Title", "Markdown Content", "Locale", "Link", "Tags"];

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-007
// Covers: parse_xlsx_wiki returns WikiPages for valid xlsx bytes with all fields.
#[test]
fn valid_wiki_xlsx_produces_pages_with_correct_fields() {
    let bytes = make_test_xlsx(
        &WIKI_HEADERS,
        &[
            vec![
                "Page One",
                "# Hello",
                "zh",
                "https://example.com/1",
                "intro,getting-started",
            ],
            vec![
                "Page Two",
                "# World",
                "en",
                "https://example.com/2",
                "advanced",
            ],
        ],
    );

    let result = parse_xlsx_wiki(&bytes).expect("should parse valid wiki xlsx");

    assert_eq!(
        result.pages.len(),
        2,
        "should produce 2 pages for 2 data rows"
    );

    // Page 0
    assert!(
        !result.pages[0].page_id.to_string().is_empty(),
        "page_id should be non-empty"
    );
    assert_eq!(result.pages[0].title, "Page One");
    assert_eq!(result.pages[0].markdown, "# Hello");
    assert_eq!(result.pages[0].locale.as_deref(), Some("zh"));
    assert_eq!(
        result.pages[0].link.as_deref(),
        Some("https://example.com/1")
    );
    assert_eq!(result.pages[0].tags, vec!["intro", "getting-started"]);

    // Page 1
    assert!(
        !result.pages[1].page_id.to_string().is_empty(),
        "page_id should be non-empty"
    );
    assert_ne!(
        result.pages[0].page_id, result.pages[1].page_id,
        "each page should have a unique page_id"
    );
    assert_eq!(result.pages[1].title, "Page Two");
    assert_eq!(result.pages[1].markdown, "# World");
    assert_eq!(result.pages[1].tags, vec!["advanced"]);
}

// User Story: US-CORE-007
// Covers: Column mapping by name, not position. Title at last column still works.
#[test]
fn column_order_independence() {
    let bytes = make_test_xlsx(
        &["Tags", "Markdown Content", "Title"],
        &[vec!["tag1", "# Content", "My Title"]],
    );

    let result = parse_xlsx_wiki(&bytes).expect("should parse regardless of column order");

    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.pages[0].title, "My Title");
    assert_eq!(result.pages[0].markdown, "# Content");
    assert_eq!(result.pages[0].tags, vec!["tag1"]);
}

// User Story: US-CORE-007
// Covers: Minimal xlsx with only Title and Markdown Content columns.
#[test]
fn minimal_xlsx_with_only_required_columns() {
    let bytes = make_test_xlsx(
        &["Title", "Markdown Content"],
        &[vec!["Minimal Page", "Some markdown text"]],
    );

    let result = parse_xlsx_wiki(&bytes).expect("should parse minimal wiki xlsx");

    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.pages[0].title, "Minimal Page");
    assert_eq!(result.pages[0].markdown, "Some markdown text");
    assert!(result.pages[0].locale.is_none());
    assert!(result.pages[0].link.is_none());
    assert!(result.pages[0].tags.is_empty());
}

// User Story: US-CORE-007
// Covers: Missing Title column -> MissingRequiredColumns error.
#[test]
fn missing_title_column_returns_error() {
    let bytes = make_test_xlsx(&["Markdown Content", "Locale"], &[vec!["# Content", "zh"]]);

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::MissingRequiredColumns { missing } => {
            assert!(
                missing.contains(&"Title".to_string()),
                "missing should contain 'Title', got: {missing:?}"
            );
        }
        other => panic!("expected MissingRequiredColumns, got: {other:?}"),
    }
}

// User Story: US-CORE-007
// Covers: Missing Markdown Content column -> MissingRequiredColumns error.
#[test]
fn missing_markdown_content_column_returns_error() {
    let bytes = make_test_xlsx(&["Title", "Locale"], &[vec!["My Page", "zh"]]);

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::MissingRequiredColumns { missing } => {
            assert!(
                missing.contains(&"Markdown Content".to_string()),
                "missing should contain 'Markdown Content', got: {missing:?}"
            );
        }
        other => panic!("expected MissingRequiredColumns, got: {other:?}"),
    }
}

// User Story: US-CORE-007
// Covers: Row with empty title -> RowValidationFailed error.
#[test]
fn row_with_empty_title_returns_validation_error() {
    let bytes = make_test_xlsx(&WIKI_HEADERS, &[vec!["", "# Has markdown", "zh", "", ""]]);

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::RowValidationFailed { errors } => {
            assert_eq!(errors.len(), 1);
            assert_eq!(errors[0].excel_row, 2, "first data row is Excel row 2");
            assert!(errors[0].missing_fields.contains(&"Title".to_string()));
        }
        other => panic!("expected RowValidationFailed, got: {other:?}"),
    }
}

// User Story: US-CORE-007
// Covers: Row with empty markdown -> RowValidationFailed error.
#[test]
fn row_with_empty_markdown_returns_validation_error() {
    let bytes = make_test_xlsx(&WIKI_HEADERS, &[vec!["Has Title", "", "zh", "", ""]]);

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::RowValidationFailed { errors } => {
            assert_eq!(errors.len(), 1);
            assert!(errors[0]
                .missing_fields
                .contains(&"Markdown Content".to_string()));
        }
        other => panic!("expected RowValidationFailed, got: {other:?}"),
    }
}

// User Story: US-CORE-007
// Covers: Multiple rows with missing fields -> reports all errors.
#[test]
fn multiple_rows_with_missing_fields_reports_all() {
    let bytes = make_test_xlsx(
        &WIKI_HEADERS,
        &[
            vec!["", "", "zh", "", ""],          // row 0: missing both
            vec!["Valid", "# OK", "", "", ""],   // row 1: valid
            vec!["No markdown", "", "", "", ""], // row 2: missing markdown
        ],
    );

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::RowValidationFailed { errors } => {
            assert_eq!(errors.len(), 2, "should report errors for row 0 and row 2");
            assert_eq!(errors[0].excel_row, 2, "first data row is Excel row 2");
            assert_eq!(errors[1].excel_row, 4, "third data row is Excel row 4");
        }
        other => panic!("expected RowValidationFailed, got: {other:?}"),
    }
}

// User Story: US-CORE-007
// Covers: Tags comma-separated parsing produces correct Vec<String>.
#[test]
fn tags_comma_separated_parsing() {
    let bytes = make_test_xlsx(
        &WIKI_HEADERS,
        &[vec!["Page", "# MD", "zh", "", " rust , web , tokio "]],
    );

    let result = parse_xlsx_wiki(&bytes).expect("should parse");
    assert_eq!(result.pages[0].tags, vec!["rust", "web", "tokio"]);
}

// User Story: US-CORE-007
// Covers: Tags empty -> empty Vec.
#[test]
fn tags_empty_produces_empty_vec() {
    let bytes = make_test_xlsx(&WIKI_HEADERS, &[vec!["Page", "# MD", "zh", "", ""]]);

    let result = parse_xlsx_wiki(&bytes).expect("should parse");
    assert!(result.pages[0].tags.is_empty());
}

// User Story: US-CORE-007
// Covers: Locale/Link empty -> None.
#[test]
fn optional_fields_empty_produces_none() {
    let bytes = make_test_xlsx(&WIKI_HEADERS, &[vec!["Page", "# MD", "", "", "tag1"]]);

    let result = parse_xlsx_wiki(&bytes).expect("should parse");
    assert!(result.pages[0].locale.is_none());
    assert!(result.pages[0].link.is_none());
}

// User Story: US-CORE-001
// Covers: Empty file (zero bytes) returns error.
#[test]
fn empty_bytes_returns_error() {
    let result = parse_xlsx_wiki(&[]);
    assert!(result.is_err(), "empty bytes should return an error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WikiParseError::InvalidFormat(ref msg) if msg.contains("Failed to parse xlsx")),
        "expected InvalidFormat with parse failure message, got: {err:?}"
    );
}

// User Story: US-CORE-001
// Covers: Malformed (non-xlsx) bytes return error.
#[test]
fn malformed_bytes_returns_error() {
    let result = parse_xlsx_wiki(b"this is definitely not an xlsx file at all");
    assert!(result.is_err(), "malformed bytes should return an error");
    let err = result.unwrap_err();
    assert!(
        matches!(err, WikiParseError::InvalidFormat(ref msg) if msg.contains("Failed to parse xlsx")),
        "expected InvalidFormat with parse failure message, got: {err:?}"
    );
}

// User Story: US-CORE-001
// Covers: File with only headers (no data rows) returns NoDataRows error.
#[test]
fn headers_only_no_data_rows_returns_error() {
    let bytes = make_test_xlsx(&["Title", "Markdown Content"], &[]);

    let result = parse_xlsx_wiki(&bytes);
    assert!(result.is_err());
    match result.unwrap_err() {
        WikiParseError::NoDataRows => {}
        other => panic!("expected NoDataRows, got: {other:?}"),
    }
}

// User Story: US-CORE-001
// Covers: File with special characters (unicode, commas, quotes) in cells
//         produces correct WikiPage fields.
#[test]
fn special_characters_in_cells_are_preserved() {
    let bytes = make_test_xlsx(
        &WIKI_HEADERS,
        &[
            vec![
                "Widget \u{2603}",
                "A \"great\" item, with commas & unicode: \u{2603}",
                "",
                "",
                "",
            ],
            vec!["Gadget", "Price is \u{00a5}1,000 (about $10)", "", "", ""],
        ],
    );

    let result = parse_xlsx_wiki(&bytes).expect("should parse xlsx with special chars");

    assert_eq!(result.pages.len(), 2);
    assert!(
        result.pages[0].title.contains("\u{2603}"),
        "unicode snowman should be preserved in title"
    );
    assert!(
        result.pages[0].markdown.contains("\"great\""),
        "quoted text should be preserved in markdown"
    );
    assert!(
        result.pages[0].markdown.contains("\u{2603}"),
        "unicode snowman should be preserved in markdown"
    );
    assert!(
        result.pages[1].markdown.contains("\u{00a5}"),
        "yen sign should be preserved in markdown"
    );
}

// User Story: US-CORE-001
// Covers: multi-sheet file reads only first sheet.
#[test]
fn multi_sheet_file_reads_only_first_sheet() {
    let mut workbook = rust_xlsxwriter::Workbook::new();

    // First sheet with Wiki data
    let ws1 = workbook.add_worksheet();
    ws1.write_string(0, 0, "Title").unwrap();
    ws1.write_string(0, 1, "Markdown Content").unwrap();
    ws1.write_string(1, 0, "Page1").unwrap();
    ws1.write_string(1, 1, "# Content from sheet 1").unwrap();

    // Second sheet with different data (should be ignored)
    let ws2 = workbook.add_worksheet();
    ws2.write_string(0, 0, "Title").unwrap();
    ws2.write_string(0, 1, "Markdown Content").unwrap();
    ws2.write_string(1, 0, "Page2").unwrap();
    ws2.write_string(1, 1, "# Content from sheet 2").unwrap();

    let bytes = workbook.save_to_buffer().unwrap().to_vec();

    let result = parse_xlsx_wiki(&bytes).expect("should parse multi-sheet xlsx");

    assert_eq!(
        result.pages.len(),
        1,
        "should only produce pages from first sheet"
    );
    assert_eq!(result.pages[0].title, "Page1");
    assert!(
        result.pages[0].markdown.contains("sheet 1"),
        "should contain first sheet data"
    );
}

// User Story: US-CORE-001
// Covers: Rows with all empty cells are skipped (do not produce pages or errors).
#[test]
fn rows_with_all_empty_cells_are_skipped() {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let ws = workbook.add_worksheet();
    ws.write_string(0, 0, "Title").unwrap();
    ws.write_string(0, 1, "Markdown Content").unwrap();
    ws.write_string(1, 0, "Page One").unwrap();
    ws.write_string(1, 1, "# First").unwrap();
    // Row 2: leave completely empty (no writes at all)
    // Row 3: another valid row
    ws.write_string(3, 0, "Page Three").unwrap();
    ws.write_string(3, 1, "# Third").unwrap();

    let bytes = workbook.save_to_buffer().unwrap().to_vec();
    let result = parse_xlsx_wiki(&bytes).expect("should parse xlsx with empty row");

    assert_eq!(
        result.pages.len(),
        2,
        "should produce 2 pages, skipping the all-empty row; got {}",
        result.pages.len(),
    );
    assert_eq!(result.pages[0].title, "Page One");
    assert_eq!(result.pages[1].title, "Page Three");
    // page_id should be non-empty and unique
    assert!(!result.pages[1].page_id.to_string().is_empty());
    assert_ne!(result.pages[0].page_id, result.pages[1].page_id);
}

// User Story: US-CORE-001
// Covers: Empty headers (xlsx with no header row content) returns error.
#[test]
fn empty_headers_returns_error() {
    // Create an xlsx with a worksheet but no content at all
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let _ws = workbook.add_worksheet();
    let bytes = workbook.save_to_buffer().unwrap().to_vec();

    let result = parse_xlsx_wiki(&bytes);
    assert!(
        result.is_err(),
        "xlsx with completely empty worksheet should return error"
    );
}

// User Story: US-CORE-007
// Covers: WikiParseError Display produces correct Chinese error messages.
#[test]
fn wiki_parse_error_display_messages() {
    let err = WikiParseError::MissingRequiredColumns {
        missing: vec!["Title".to_string(), "Markdown Content".to_string()],
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("Missing required column(s)"),
        "MissingRequiredColumns display: {msg}"
    );
    assert!(
        msg.contains("Title"),
        "MissingRequiredColumns display: {msg}"
    );
    assert!(
        msg.contains("Markdown Content"),
        "MissingRequiredColumns display: {msg}"
    );

    let err = WikiParseError::RowValidationFailed {
        errors: vec![super::xlsx_parser::RowValidationError {
            excel_row: 2,
            missing_fields: vec!["Title".to_string()],
        }],
    };
    let msg = format!("{err}");
    assert!(
        msg.contains("incomplete"),
        "RowValidationFailed display: {msg}"
    );
    // excel_row 2 -> display row 2 directly (already 1-based)
    assert!(
        msg.contains("Row 2"),
        "RowValidationFailed display row number: {msg}"
    );

    let err = WikiParseError::NoDataRows;
    let msg = format!("{err}");
    assert!(
        msg.contains("No usable data rows"),
        "NoDataRows display: {msg}"
    );

    let err = WikiParseError::InvalidFormat("test error".to_string());
    let msg = format!("{err}");
    assert!(
        msg.contains("Invalid file format"),
        "InvalidFormat display: {msg}"
    );
    assert!(msg.contains("test error"), "InvalidFormat display: {msg}");
}
