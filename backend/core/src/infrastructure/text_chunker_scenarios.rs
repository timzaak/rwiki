//! Scenario tests for text_chunker chunk_count + char limit integration.
//!
//! Covers the integrated split + fill pipeline via `split_long_chunks_with_section`,
//! verifying that chunk_count is correctly populated after the full pipeline
//! and that the DEFAULT_MAX_CHUNK_CHARS = 1600 limit is applied by default.
//!
//! These tests complement the unit tests in text_chunker.rs which call
//! fill_chunk_counts() in isolation. Here we exercise the complete
//! split_long_chunks_with_section function end-to-end.

use super::text_chunker::{split_long_chunks_with_section, split_long_chunks_with_section_default};
use super::xlsx_parser::ParsedChunk;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

// User Story: US-CORE-001
// Covers: When a single row's content exceeds max_chunk_chars and is split into
//         N sub-chunks via split_long_chunks_with_section, each resulting sub-chunk
//         has chunk_count = Some(N). This verifies the integrated split + fill
//         pipeline produces correct chunk_count values, not just the isolated
//         fill_chunk_counts function.
#[test]
fn chunk_count_for_split_row_sub_chunks() {
    // Build content that will split into at least 3 sub-chunks at max_chunk_chars = 200
    let long_content: String = (0..15)
        .map(|i| format!("## Section {i}\n\nParagraph text for section {i}.\n\n"))
        .collect();

    let pid = Uuid::now_v7();
    let chunks = vec![ParsedChunk {
        content: long_content,
        page_id: pid,
        ..Default::default()
    }];

    let result = split_long_chunks_with_section(chunks, 200);

    assert!(
        result.len() >= 3,
        "long content should produce at least 3 sub-chunks, got {}",
        result.len()
    );

    let expected_count = result.len();
    for (i, sub) in result.iter().enumerate() {
        assert_eq!(sub.page_id, pid, "all sub-chunks should preserve page_id");
        assert_eq!(
            sub.chunk_count,
            Some(expected_count),
            "sub-chunk {} should have chunk_count = Some({}), got {:?}",
            i,
            expected_count,
            sub.chunk_count
        );
    }
}

// User Story: US-CORE-001
// Covers: A short chunk that is not split gets chunk_count = Some(1) after
//         passing through split_long_chunks_with_section. This verifies that
//         fill_chunk_counts correctly annotates single (unsplit) chunks.
#[test]
fn chunk_count_unsplit_single_chunk_is_one() {
    let chunks = vec![ParsedChunk {
        content: "Short text that fits easily.".to_string(),
        page_id: Uuid::now_v7(),
        ..Default::default()
    }];

    let result = split_long_chunks_with_section(chunks, 200);

    assert_eq!(result.len(), 1, "short chunk should not be split");
    assert_eq!(
        result[0].chunk_count,
        Some(1),
        "single unsplit chunk should have chunk_count = Some(1)"
    );
}

// User Story: US-CORE-001
// Covers: Multiple rows each independently compute their chunk_count. A long
//         row split into N sub-chunks has chunk_count = Some(N), a short row
//         has chunk_count = Some(1), and values from different rows do not leak.
#[test]
fn chunk_count_independent_per_row() {
    let pid0 = Uuid::now_v7();
    let pid1 = Uuid::now_v7();
    let pid2 = Uuid::now_v7();

    // Page 0: long content -> multiple sub-chunks
    let long_content: String = (0..15)
        .map(|i| format!("## Heading {i}\n\nParagraph content for {i}.\n\n"))
        .collect();

    // Page 1: short content -> single chunk
    let short_content = "Short text.".to_string();

    // Page 2: medium-long content -> at least 2 sub-chunks (needs >200 chars)
    let medium_content: String = (0..10)
        .map(|i| format!("## Sub {i}\nSome medium text here.\n"))
        .collect();

    let chunks = vec![
        ParsedChunk {
            content: long_content,
            page_id: pid0,
            ..Default::default()
        },
        ParsedChunk {
            content: short_content,
            page_id: pid1,
            ..Default::default()
        },
        ParsedChunk {
            content: medium_content,
            page_id: pid2,
            ..Default::default()
        },
    ];

    let result = split_long_chunks_with_section(chunks, 200);

    // Count sub-chunks per page
    let page0_count = result.iter().filter(|c| c.page_id == pid0).count();
    let page1_count = result.iter().filter(|c| c.page_id == pid1).count();
    let page2_count = result.iter().filter(|c| c.page_id == pid2).count();

    assert!(
        page0_count >= 3,
        "page 0 should produce >= 3 sub-chunks, got {}",
        page0_count
    );
    assert_eq!(page1_count, 1, "page 1 should produce exactly 1 chunk");
    assert!(
        page2_count >= 2,
        "page 2 should produce >= 2 sub-chunks, got {}",
        page2_count
    );

    // Verify chunk_count per page
    for sub in result.iter().filter(|c| c.page_id == pid0) {
        assert_eq!(
            sub.chunk_count,
            Some(page0_count),
            "page 0 sub-chunk should have chunk_count = Some({})",
            page0_count
        );
    }

    for sub in result.iter().filter(|c| c.page_id == pid1) {
        assert_eq!(
            sub.chunk_count,
            Some(1),
            "page 1 chunk should have chunk_count = Some(1)"
        );
    }

    for sub in result.iter().filter(|c| c.page_id == pid2) {
        assert_eq!(
            sub.chunk_count,
            Some(page2_count),
            "page 2 sub-chunk should have chunk_count = Some({})",
            page2_count
        );
    }
}

// User Story: US-CORE-001
// Covers: split_long_chunks_with_section_default uses the 1600-char limit.
//         Content of length 1400 (< 1600) passes through unsplit. Under the
//         old limit (1200) this would have split; under the new limit (1600)
//         it should remain a single chunk with sub_index = None.
#[test]
fn char_limit_1600_used_by_default_function() {
    // Build content between 1200 and 1600 characters
    let content_1400: String = (0..70).map(|i| format!("Word{} ", i)).collect();
    // Pad to ensure we're in the 1200-1600 range but under 1600
    let content_1400 = {
        let mut s = content_1400;
        while s.len() < 1400 {
            s.push_str("padding text. ");
        }
        s
    };

    assert!(
        content_1400.len() >= 1200,
        "test content should be >= 1200 chars, got {}",
        content_1400.len()
    );
    assert!(
        content_1400.len() < 1600,
        "test content should be < 1600 chars, got {}",
        content_1400.len()
    );

    let chunks = vec![ParsedChunk {
        content: content_1400,
        page_id: Uuid::now_v7(),
        ..Default::default()
    }];

    let result = split_long_chunks_with_section_default(chunks);

    assert_eq!(
        result.len(),
        1,
        "content under 1600 chars should pass through unsplit, got {} chunks",
        result.len()
    );
    assert!(
        result[0].sub_index.is_none(),
        "unsplit chunk should have sub_index = None"
    );
    assert_eq!(
        result[0].chunk_count,
        Some(1),
        "unsplit chunk should have chunk_count = Some(1)"
    );
}
