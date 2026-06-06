use rig::Embed;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::xlsx_parser::{ContentType, ParsedChunk};

/// Embeddable document chunk for rig-core's vector store.
///
/// Each chunk represents one row of parsed xlsx data, converted to text
/// for embedding via the `Embed` derive with the `content` field.
#[derive(Embed, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentChunk {
    pub id: String,
    pub document_id: String,
    pub page_id: String,
    /// Sub-index for chunks split from a single row (None for original chunks)
    pub sub_index: Option<usize>,
    #[embed]
    pub content: Vec<String>,
    // Metadata fields (not embedded, do not participate in embedding calculation)
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    pub tags: Vec<String>,
    pub section: Option<String>,
    /// Number of chunks the original row was split into (None for original/unsplit chunks)
    pub chunk_count: Option<usize>,
    /// Document content type for tokenizer strategy routing
    pub content_type: ContentType,
    /// Pre-computed FTS tokenization result
    pub fts_tokens: Option<String>,
}

impl DocumentChunk {
    /// Convert a parsed xlsx row chunk into an embeddable DocumentChunk.
    pub fn from_parsed(document_id: Uuid, parsed: &ParsedChunk) -> Self {
        let id = match parsed.sub_index {
            Some(sub_idx) => format!("{}:{}", parsed.page_id, sub_idx),
            None => format!("{}:0", parsed.page_id),
        };
        Self {
            id,
            document_id: document_id.to_string(),
            page_id: parsed.page_id.to_string(),
            sub_index: parsed.sub_index,
            content: vec![parsed.content.clone()],
            title: parsed.title.clone(),
            locale: parsed.locale.clone(),
            link: parsed.link.clone(),
            tags: parsed.tags.clone(),
            section: parsed.section.clone(),
            chunk_count: parsed.chunk_count,
            content_type: parsed.content_type.clone(),
            fts_tokens: parsed.fts_tokens.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_parsed_without_sub_index_generates_simple_id() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "some text".to_string(),
            page_id,
            sub_index: None,
            title: "Test Title".to_string(),
            locale: Some("en".to_string()),
            link: Some("https://example.com".to_string()),
            tags: vec!["tag1".to_string()],
            section: Some("Intro".to_string()),
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.id,
            format!("{page_id}:0"),
            "ID without sub_index should be '{{page_id}}:0'"
        );
        assert_eq!(chunk.page_id, page_id.to_string());
        assert!(chunk.sub_index.is_none());
        assert_eq!(chunk.content, vec!["some text"]);
        assert_eq!(chunk.title, "Test Title");
        assert_eq!(chunk.locale.as_deref(), Some("en"));
        assert_eq!(chunk.link.as_deref(), Some("https://example.com"));
        assert_eq!(chunk.tags, vec!["tag1"]);
        assert_eq!(chunk.section.as_deref(), Some("Intro"));
    }

    #[test]
    fn from_parsed_with_sub_index_generates_triple_id() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "split text".to_string(),
            page_id,
            sub_index: Some(2),
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.id,
            format!("{page_id}:2"),
            "ID with sub_index should be '{{page_id}}:{{sub_index}}'"
        );
        assert_eq!(chunk.page_id, page_id.to_string());
        assert_eq!(chunk.sub_index, Some(2));
        assert_eq!(chunk.content, vec!["split text"]);
        assert_eq!(chunk.title, "", "default title should be empty string");
        assert!(chunk.locale.is_none());
        assert!(chunk.link.is_none());
        assert!(chunk.tags.is_empty());
        assert!(chunk.section.is_none());
    }

    #[test]
    fn from_parsed_propagates_metadata_fields() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "wiki content".to_string(),
            page_id,
            title: "Wiki Page Title".to_string(),
            locale: Some("zh".to_string()),
            link: Some("https://wiki.example.com/page".to_string()),
            tags: vec!["rust".to_string(), "backend".to_string()],
            section: Some("Architecture".to_string()),
            chunk_count: Some(3),
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(chunk.title, "Wiki Page Title");
        assert_eq!(chunk.locale.as_deref(), Some("zh"));
        assert_eq!(chunk.link.as_deref(), Some("https://wiki.example.com/page"));
        assert_eq!(chunk.tags, vec!["rust", "backend"]);
        assert_eq!(chunk.section.as_deref(), Some("Architecture"));
        assert_eq!(chunk.chunk_count, Some(3));
    }

    // User Story: US-CORE-001
    // Covers: DocumentChunk::from_parsed() propagates the chunk_count field.
    //         A ParsedChunk with chunk_count = Some(3) must produce a
    //         DocumentChunk with chunk_count = Some(3). This is a focused
    //         regression guard for the chunk_count field specifically.
    #[test]
    fn from_parsed_propagates_chunk_count() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "split text".to_string(),
            page_id,
            sub_index: Some(1),
            chunk_count: Some(3),
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.chunk_count,
            Some(3),
            "from_parsed must propagate chunk_count from ParsedChunk"
        );
        assert_eq!(chunk.page_id, page_id.to_string());
        assert_eq!(chunk.sub_index, Some(1));
    }

    // User Story: US-CORE-027
    // Covers: DocumentChunk::from_parsed() propagates content_type from ParsedChunk.
    //         A ParsedChunk with content_type: ContentType::OpenApi must produce
    //         a DocumentChunk with content_type: ContentType::OpenApi.
    #[test]
    fn from_parsed_propagates_content_type() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "## GET /api/health".to_string(),
            page_id,
            content_type: ContentType::OpenApi,
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.content_type,
            ContentType::OpenApi,
            "from_parsed must propagate content_type: ContentType::OpenApi from ParsedChunk"
        );
    }

    // User Story: US-CORE-027
    // Covers: DocumentChunk::from_parsed() propagates fts_tokens from ParsedChunk.
    //         A ParsedChunk with fts_tokens: Some("POST api documents") must produce
    //         a DocumentChunk with the same fts_tokens value.
    #[test]
    fn from_parsed_propagates_fts_tokens() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "## POST /api/documents".to_string(),
            page_id,
            content_type: ContentType::OpenApi,
            fts_tokens: Some("POST api documents".to_string()),
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.fts_tokens,
            Some("POST api documents".to_string()),
            "from_parsed must propagate fts_tokens from ParsedChunk"
        );
    }

    // Covers: A ParsedChunk created with ..Default::default() has content_type: ContentType::None
    //         and fts_tokens: None. The resulting DocumentChunk must also have these defaults.
    #[test]
    fn from_parsed_default_content_type_is_none() {
        let doc_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3c").unwrap();
        let page_id = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2b3d").unwrap();
        let parsed = ParsedChunk {
            content: "plain text".to_string(),
            page_id,
            ..Default::default()
        };

        let chunk = DocumentChunk::from_parsed(doc_id, &parsed);

        assert_eq!(
            chunk.content_type,
            ContentType::None,
            "default content_type should be ContentType::None"
        );
        assert!(
            chunk.fts_tokens.is_none(),
            "default fts_tokens should be None"
        );
    }
}
