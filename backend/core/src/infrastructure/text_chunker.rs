use text_splitter::MarkdownSplitter;
use uuid::Uuid;

use super::xlsx_parser::ParsedChunk;

const DEFAULT_MAX_CHUNK_CHARS: usize = 1600;

/// Split chunks whose content exceeds `max_chunk_chars` using Markdown-aware splitting.
///
/// Chunks that fit within the limit are passed through unchanged.
/// Oversized chunks are split into sub-chunks, each annotated with an incrementing `sub_index`.
pub fn split_long_chunks(chunks: Vec<ParsedChunk>, max_chunk_chars: usize) -> Vec<ParsedChunk> {
    let splitter = MarkdownSplitter::new(max_chunk_chars);
    chunks
        .into_iter()
        .flat_map(|chunk| {
            if chunk.content.len() <= max_chunk_chars {
                vec![chunk]
            } else {
                splitter
                    .chunks(&chunk.content)
                    .enumerate()
                    .map(|(sub_idx, text)| ParsedChunk {
                        content: text.to_string(),
                        sub_index: Some(sub_idx),
                        ..chunk.clone()
                    })
                    .collect()
            }
        })
        .collect()
}

/// Convenience wrapper using the default 1600-character limit.
pub fn split_long_chunks_default(chunks: Vec<ParsedChunk>) -> Vec<ParsedChunk> {
    split_long_chunks(chunks, DEFAULT_MAX_CHUNK_CHARS)
}

/// Extract Markdown headings (lines starting with #) as `(byte_offset, heading_text)` pairs.
///
/// Uses line-based scanning of the raw markdown source rather than pulldown-cmark event offsets,
/// because pulldown-cmark's `Parser` does not expose source byte positions. Both MarkdownSplitter
/// and this function operate on the raw string, so byte offsets are directly comparable.
fn extract_headings(markdown: &str) -> Vec<(usize, String)> {
    let mut headings = Vec::new();
    let mut byte_offset = 0;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if (1..=6).contains(&level) {
                let text = rest.trim_start_matches('#').trim().to_string();
                if !text.is_empty() {
                    headings.push((byte_offset, text));
                }
            }
        }
        byte_offset += line.len() + 1; // +1 for newline
    }
    headings
}

/// Find the section heading for a sub-chunk at `sub_chunk_start` byte offset in the original markdown.
///
/// Returns the text of the nearest heading whose position is at or before `sub_chunk_start`.
fn find_section_for_chunk(headings: &[(usize, String)], sub_chunk_start: usize) -> Option<String> {
    headings
        .iter()
        .rfind(|(offset, _)| *offset <= sub_chunk_start)
        .map(|(_, text)| text.clone())
}

/// Split chunks whose content exceeds `max_chunk_chars`, tracking Markdown heading sections.
///
/// Like `split_long_chunks`, but also populates the `section` field:
/// - For long chunks split into sub-chunks, each sub-chunk's section is the nearest preceding
///   heading in the original markdown.
/// - For short chunks that pass through, the section is extracted from the first heading in the
///   content, if any.
///
/// Sub-chunks inherit `title`, `locale`, `link`, and `tags` from the parent chunk.
pub fn split_long_chunks_with_section(
    chunks: Vec<ParsedChunk>,
    max_chunk_chars: usize,
) -> Vec<ParsedChunk> {
    let splitter = MarkdownSplitter::new(max_chunk_chars);
    let mut result: Vec<ParsedChunk> = chunks
        .into_iter()
        .flat_map(|chunk| {
            if chunk.content.len() <= max_chunk_chars {
                // Short chunk: extract section from first heading if present
                let section = extract_headings(&chunk.content)
                    .into_iter()
                    .next()
                    .map(|(_, text)| text);
                let mut c = chunk;
                c.section = section.or(c.section);
                vec![c]
            } else {
                let headings = extract_headings(&chunk.content);
                // Build byte-offset map for each sub-chunk by tracking cumulative position
                let full_text = chunk.content.clone();
                let mut search_offset = 0;
                splitter
                    .chunks(&full_text)
                    .enumerate()
                    .map(|(sub_idx, text)| {
                        let text_str: &str = text;
                        // Find sub-chunk start position in original text, searching forward
                        let sub_start = full_text[search_offset..]
                            .find(text_str)
                            .map(|pos| search_offset + pos)
                            .unwrap_or(search_offset);
                        search_offset = sub_start + text_str.len();
                        let section = find_section_for_chunk(&headings, sub_start);
                        ParsedChunk {
                            content: text.to_string(),
                            sub_index: Some(sub_idx),
                            section,
                            ..chunk.clone()
                        }
                    })
                    .collect()
            }
        })
        .collect();
    fill_chunk_counts(&mut result);
    result
}

/// Fill `chunk_count` on each chunk to the total number of sub-chunks sharing the same `page_id`.
fn fill_chunk_counts(chunks: &mut [ParsedChunk]) {
    use std::collections::HashMap;
    let mut counts: HashMap<Uuid, usize> = HashMap::new();
    for chunk in chunks.iter() {
        *counts.entry(chunk.page_id).or_insert(0) += 1;
    }
    for chunk in chunks.iter_mut() {
        let total = counts[&chunk.page_id];
        chunk.chunk_count = Some(total);
    }
}

/// Convenience wrapper for `split_long_chunks_with_section` using the default 1600-character limit.
pub fn split_long_chunks_with_section_default(chunks: Vec<ParsedChunk>) -> Vec<ParsedChunk> {
    split_long_chunks_with_section(chunks, DEFAULT_MAX_CHUNK_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Short text stays within the limit and passes through unchanged.
    /// sub_index remains None for unmodified chunks.
    #[test]
    fn short_chunks_pass_through_unchanged() {
        let pid0 = uuid::Uuid::now_v7();
        let pid1 = uuid::Uuid::now_v7();
        let chunks = vec![
            ParsedChunk {
                content: "short text".to_string(),
                page_id: pid0,
                ..Default::default()
            },
            ParsedChunk {
                content: "another short one".to_string(),
                page_id: pid1,
                ..Default::default()
            },
        ];

        let result = split_long_chunks(chunks.clone(), 200);

        assert_eq!(result.len(), 2, "short chunks should not be split");
        for (i, chunk) in result.iter().enumerate() {
            assert_eq!(chunk.content, chunks[i].content);
            assert_eq!(chunk.page_id, chunks[i].page_id);
            assert!(
                chunk.sub_index.is_none(),
                "unmodified chunks should keep sub_index = None"
            );
        }
    }

    /// Long markdown with headings gets split, each sub-chunk within max_chunk_chars,
    /// and sub_index increments sequentially from 0.
    #[test]
    fn long_chunks_are_split_into_sub_chunks() {
        // Build a long markdown string with headings that exceeds max_chunk_chars
        let long_content: String = (0..20)
            .map(|i| format!("## Heading {i}\n\nSome paragraph text here.\n\n"))
            .collect();

        let pid = uuid::Uuid::now_v7();
        let max_chars = 200;
        let chunks = vec![ParsedChunk {
            content: long_content,
            page_id: pid,
            ..Default::default()
        }];

        let result = split_long_chunks(chunks, max_chars);

        assert!(
            result.len() > 1,
            "long chunk should be split into multiple sub-chunks, got {}",
            result.len()
        );

        for (i, sub) in result.iter().enumerate() {
            assert!(
                sub.content.len() <= max_chars,
                "sub-chunk {i} exceeds max_chunk_chars: {} > {max_chars}",
                sub.content.len()
            );
            assert_eq!(
                sub.page_id, pid,
                "all sub-chunks should preserve original page_id"
            );
            assert_eq!(
                sub.sub_index,
                Some(i),
                "sub_index should be sequential from 0"
            );
        }
    }

    /// Mix of short and long chunks: short ones pass through, long ones get split.
    /// Original page_id is preserved across split results.
    #[test]
    fn mixed_short_and_long_chunks() {
        let pid0 = uuid::Uuid::now_v7();
        let pid1 = uuid::Uuid::now_v7();
        let pid2 = uuid::Uuid::now_v7();

        let long_text: String = (0..15)
            .map(|i| format!("## Section {i}\n\nParagraph content.\n\n"))
            .collect();

        let chunks = vec![
            ParsedChunk {
                content: "short".to_string(),
                page_id: pid0,
                ..Default::default()
            },
            ParsedChunk {
                content: long_text,
                page_id: pid1,
                ..Default::default()
            },
            ParsedChunk {
                content: "also short".to_string(),
                page_id: pid2,
                ..Default::default()
            },
        ];

        let result = split_long_chunks(chunks, 200);

        // First chunk (short) should pass through unchanged
        assert_eq!(result[0].content, "short");
        assert_eq!(result[0].page_id, pid0);
        assert!(result[0].sub_index.is_none());

        // Last chunk (short) should also pass through
        let last = result.last().expect("should have at least 3 results");
        assert_eq!(last.content, "also short");
        assert_eq!(last.page_id, pid2);
        assert!(last.sub_index.is_none());

        // Middle (long) chunk should be split and all sub-chunks have page_id = pid1
        let middle_sub_chunks: Vec<_> = result.iter().filter(|c| c.page_id == pid1).collect();
        assert!(
            middle_sub_chunks.len() > 1,
            "long chunk with pid1 should be split into multiple sub-chunks"
        );
        for sub in &middle_sub_chunks {
            assert_eq!(sub.page_id, pid1);
        }
    }

    /// Verify sub_index increments correctly from 0 for a single long chunk.
    #[test]
    fn sub_index_increments_correctly() {
        let long_content: String = (0..30)
            .map(|i| format!("### Sub {i}\nSome text.\n"))
            .collect();

        let chunks = vec![ParsedChunk {
            content: long_content,
            page_id: uuid::Uuid::now_v7(),
            ..Default::default()
        }];

        let result = split_long_chunks(chunks, 100);

        assert!(result.len() > 1, "should produce multiple sub-chunks");

        for (i, sub) in result.iter().enumerate() {
            assert_eq!(
                sub.sub_index,
                Some(i),
                "sub_index at position {i} should be Some({i})"
            );
        }
    }

    // ---------------------------------------------------------------------------
    // Section tracking tests
    // ---------------------------------------------------------------------------

    /// Short markdown with a heading gets section from the first heading.
    #[test]
    fn section_tracking_short_chunk_with_heading() {
        let chunks = vec![ParsedChunk {
            content: "## Introduction\n\nSome intro text.".to_string(),
            page_id: uuid::Uuid::now_v7(),
            ..Default::default()
        }];

        let result = split_long_chunks_with_section(chunks, 1200);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].section.as_deref(),
            Some("Introduction"),
            "short chunk with heading should have section = heading text"
        );
    }

    /// Short markdown without headings has section = None.
    #[test]
    fn section_tracking_short_chunk_without_heading() {
        let chunks = vec![ParsedChunk {
            content: "Just some plain text without headings.".to_string(),
            page_id: uuid::Uuid::now_v7(),
            ..Default::default()
        }];

        let result = split_long_chunks_with_section(chunks, 1200);

        assert_eq!(result.len(), 1);
        assert!(
            result[0].section.is_none(),
            "short chunk without headings should have section = None"
        );
    }

    /// Long markdown split into sub-chunks: each sub-chunk gets the nearest preceding heading.
    #[test]
    fn section_tracking_long_chunk_split_gets_sections() {
        let long_markdown = "\
## Alpha section about algorithms

This section discusses fundamental algorithm design principles including \
divide and conquer, dynamic programming, and greedy strategies. We explore \
time complexity analysis, Big-O notation, and amortized analysis techniques \
that help engineers evaluate and compare algorithm performance across \
different problem domains and input distributions.

## Beta section about databases

This section covers relational database design, normalization forms, and \
query optimization strategies. Topics include indexing, query planning, \
transaction isolation levels, and distributed database consistency models \
that ensure data integrity in concurrent and partitioned environments \
with various replication and sharding approaches.

## Gamma section about networking

This section explains network protocols, the OSI model, and transport layer \
mechanisms. We examine TCP congestion control, UDP datagram delivery, and \
modern application-layer protocols like HTTP/2 and QUIC that improve \
web performance through multiplexing and header compression techniques.
";

        let pid = uuid::Uuid::now_v7();
        let chunks = vec![ParsedChunk {
            content: long_markdown.to_string(),
            page_id: pid,
            title: "Test Doc".to_string(),
            ..Default::default()
        }];

        let result = split_long_chunks_with_section(chunks, 300);

        assert!(
            result.len() >= 3,
            "long chunk should be split into at least 3 sub-chunks, got {}",
            result.len()
        );

        // All sub-chunks inherit title from parent
        for sub in &result {
            assert_eq!(
                sub.title, "Test Doc",
                "sub-chunk should inherit title from parent"
            );
            assert_eq!(sub.page_id, pid, "sub-chunk should preserve page_id");
            assert!(sub.sub_index.is_some(), "split chunk should have sub_index");
        }

        // At least one sub-chunk should have a section containing "Alpha"
        let alpha_sections: Vec<_> = result
            .iter()
            .filter(|c| c.section.as_deref() == Some("Alpha section about algorithms"))
            .collect();
        assert!(
            !alpha_sections.is_empty(),
            "at least one sub-chunk should have 'Alpha section about algorithms' as section"
        );

        // At least one sub-chunk should have a section containing "Beta"
        let beta_sections: Vec<_> = result
            .iter()
            .filter(|c| c.section.as_deref() == Some("Beta section about databases"))
            .collect();
        assert!(
            !beta_sections.is_empty(),
            "at least one sub-chunk should have 'Beta section about databases' as section"
        );
    }

    /// Multiple heading levels: section tracks the nearest preceding heading.
    #[test]
    fn section_tracking_multiple_heading_levels() {
        let content = "\
# Main Title

Some intro text.

## Subtitle A

Content under A.

### Sub-subtitle A1

Detailed content under A1.
";

        let chunks = vec![ParsedChunk {
            content: content.to_string(),
            page_id: uuid::Uuid::now_v7(),
            ..Default::default()
        }];

        let result = split_long_chunks_with_section(chunks, 1200);

        assert_eq!(result.len(), 1, "short content should not be split");
        assert_eq!(
            result[0].section.as_deref(),
            Some("Main Title"),
            "short chunk should get section from first heading"
        );
    }

    /// Existing section on a chunk is preserved if no heading is found in content.
    #[test]
    fn section_tracking_preserves_existing_section_when_no_heading() {
        let chunks = vec![ParsedChunk {
            content: "No heading here.".to_string(),
            page_id: uuid::Uuid::now_v7(),
            section: Some("Pre-existing section".to_string()),
            ..Default::default()
        }];

        let result = split_long_chunks_with_section(chunks, 1200);

        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].section.as_deref(),
            Some("Pre-existing section"),
            "existing section should be preserved when no heading in content"
        );
    }

    // ---------------------------------------------------------------------------
    // fill_chunk_counts tests
    // ---------------------------------------------------------------------------

    /// Multiple sub-chunks from the same page all get chunk_count = N (the total for that page).
    #[test]
    fn fill_chunk_counts_sets_total_for_split_row() {
        let pid = uuid::Uuid::now_v7();
        let mut chunks = vec![
            ParsedChunk {
                content: "a".into(),
                page_id: pid,
                ..Default::default()
            },
            ParsedChunk {
                content: "b".into(),
                page_id: pid,
                ..Default::default()
            },
            ParsedChunk {
                content: "c".into(),
                page_id: pid,
                ..Default::default()
            },
        ];
        fill_chunk_counts(&mut chunks);
        for chunk in &chunks {
            assert_eq!(
                chunk.chunk_count,
                Some(3),
                "all sub-chunks of the same page should have chunk_count = 3"
            );
        }
    }

    /// A single unsplit chunk gets chunk_count = 1.
    #[test]
    fn fill_chunk_counts_single_unsplit_chunk() {
        let mut chunks = vec![ParsedChunk {
            content: "solo".into(),
            page_id: uuid::Uuid::now_v7(),
            ..Default::default()
        }];
        fill_chunk_counts(&mut chunks);
        assert_eq!(
            chunks[0].chunk_count,
            Some(1),
            "single chunk should have chunk_count = 1"
        );
    }

    /// Chunks from different pages are counted independently.
    #[test]
    fn fill_chunk_counts_independent_per_row() {
        let pid0 = uuid::Uuid::now_v7();
        let pid1 = uuid::Uuid::now_v7();
        let pid2 = uuid::Uuid::now_v7();
        let mut chunks = vec![
            ParsedChunk {
                content: "r0-a".into(),
                page_id: pid0,
                ..Default::default()
            },
            ParsedChunk {
                content: "r1-a".into(),
                page_id: pid1,
                ..Default::default()
            },
            ParsedChunk {
                content: "r1-b".into(),
                page_id: pid1,
                ..Default::default()
            },
            ParsedChunk {
                content: "r2-a".into(),
                page_id: pid2,
                ..Default::default()
            },
            ParsedChunk {
                content: "r2-b".into(),
                page_id: pid2,
                ..Default::default()
            },
            ParsedChunk {
                content: "r2-c".into(),
                page_id: pid2,
                ..Default::default()
            },
        ];
        fill_chunk_counts(&mut chunks);
        assert_eq!(chunks[0].chunk_count, Some(1), "page 0 has 1 chunk");
        assert_eq!(chunks[1].chunk_count, Some(2), "page 1 has 2 chunks");
        assert_eq!(chunks[2].chunk_count, Some(2), "page 1 has 2 chunks");
        assert_eq!(chunks[3].chunk_count, Some(3), "page 2 has 3 chunks");
        assert_eq!(chunks[4].chunk_count, Some(3), "page 2 has 3 chunks");
        assert_eq!(chunks[5].chunk_count, Some(3), "page 2 has 3 chunks");
    }
}
