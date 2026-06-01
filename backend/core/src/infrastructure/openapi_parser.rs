//! OpenAPI 3.x JSON file parser.
//!
//! Parses an OpenAPI 3.x JSON file into `ParsedChunk` objects, one per API
//! endpoint (path + method combination). Each chunk contains a Markdown
//! representation of the endpoint for embedding and search.

use std::collections::HashMap;

use serde_json::Value;
use uuid::Uuid;

use super::xlsx_parser::ParsedChunk;

/// Recognized HTTP methods in OpenAPI paths objects.
const METHODS: &[&str] = &[
    "get", "post", "put", "delete", "patch", "options", "head", "trace",
];

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Error type for OpenAPI JSON file parsing.
#[derive(Debug, thiserror::Error)]
pub enum OpenApiParseError {
    #[error("文件编码不支持，仅支持 UTF-8")]
    InvalidEncoding,
    #[error("JSON 格式无效: {0}")]
    InvalidJson(String),
    #[error("不是有效的 OpenAPI 3.x 格式: 缺少 openapi 字段")]
    MissingOpenApiField,
    #[error("不是有效的 OpenAPI 3.x 格式: 仅支持 OpenAPI 3.x 版本")]
    UnsupportedVersion,
    #[error("文件中没有可解析的 API 端点")]
    EmptyPaths,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Parse an OpenAPI 3.x JSON file into one `ParsedChunk` per endpoint.
pub fn parse_openapi_file(
    bytes: &[u8],
    _file_name: &str,
) -> Result<Vec<ParsedChunk>, OpenApiParseError> {
    // UTF-8 decode
    let utf8_str = std::str::from_utf8(bytes).map_err(|_| OpenApiParseError::InvalidEncoding)?;

    // JSON parse
    let value: Value = serde_json::from_str(utf8_str)
        .map_err(|e| OpenApiParseError::InvalidJson(e.to_string()))?;

    // Validate openapi field
    let version = value
        .get("openapi")
        .ok_or(OpenApiParseError::MissingOpenApiField)?;
    let version_str = version
        .as_str()
        .ok_or(OpenApiParseError::MissingOpenApiField)?;
    if !version_str.starts_with("3.") {
        return Err(OpenApiParseError::UnsupportedVersion);
    }

    // Validate paths
    let paths = value
        .get("paths")
        .and_then(|p| p.as_object())
        .ok_or(OpenApiParseError::EmptyPaths)?;
    if paths.is_empty() {
        return Err(OpenApiParseError::EmptyPaths);
    }

    // Pre-extract component schemas for $ref resolution
    let schemas: HashMap<String, &Value> = value
        .get("components")
        .and_then(|c| c.get("schemas"))
        .and_then(|s| s.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v)).collect())
        .unwrap_or_default();

    let mut chunks = Vec::new();

    for (path, path_item) in paths {
        let path_item_obj = match path_item.as_object() {
            Some(obj) => obj,
            None => continue,
        };

        for &method in METHODS {
            let operation = match path_item_obj.get(method) {
                Some(op) => op,
                None => continue,
            };

            let content = format_endpoint(method, path, operation, &schemas);
            let op_obj = operation.as_object();

            // Title: operationId > summary > path
            let title = op_obj
                .and_then(|o| o.get("operationId"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    op_obj
                        .and_then(|o| o.get("summary"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| path.to_string());

            // Tags
            let tags: Vec<String> = op_obj
                .and_then(|o| o.get("tags"))
                .and_then(|t| t.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            // Section: first tag if present
            let section = tags.first().cloned();

            chunks.push(ParsedChunk {
                content,
                page_id: Uuid::now_v7(),
                sub_index: None,
                title,
                locale: None,
                link: None,
                tags,
                section,
                chunk_count: None,
            });
        }
    }

    if chunks.is_empty() {
        return Err(OpenApiParseError::EmptyPaths);
    }

    Ok(chunks)
}

// ---------------------------------------------------------------------------
// Markdown formatting
// ---------------------------------------------------------------------------

/// Format a single endpoint as Markdown.
fn format_endpoint(
    method: &str,
    path: &str,
    operation: &Value,
    schemas: &HashMap<String, &Value>,
) -> String {
    let mut md = String::new();
    let op = operation.as_object();

    // Heading
    md.push_str(&format!("## {} {}\n\n", method.to_uppercase(), path));

    // Summary
    if let Some(summary) = op.and_then(|o| o.get("summary")).and_then(|v| v.as_str()) {
        md.push_str(summary);
        md.push('\n');
    }

    // Description
    if let Some(desc) = op
        .and_then(|o| o.get("description"))
        .and_then(|v| v.as_str())
    {
        if !desc.is_empty() {
            md.push('\n');
            md.push_str(desc);
            md.push('\n');
        }
    }

    // Parameters table
    if let Some(params) = op
        .and_then(|o| o.get("parameters"))
        .and_then(|v| v.as_array())
    {
        if !params.is_empty() {
            md.push_str("\n### Parameters\n\n");
            md.push_str("| Name | In | Type | Required | Description |\n");
            md.push_str("|------|------|------|----------|-------------|\n");
            for param in params {
                let p = param.as_object();
                let name = p
                    .and_then(|o| o.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let in_val = p
                    .and_then(|o| o.get("in"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let typ = p
                    .and_then(|o| o.get("schema"))
                    .and_then(|s| s.get("type"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                let required = p
                    .and_then(|o| o.get("required"))
                    .and_then(|v| v.as_bool())
                    .map(|b| if b { "Yes" } else { "No" })
                    .unwrap_or("No");
                let desc = p
                    .and_then(|o| o.get("description"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                md.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    name, in_val, typ, required, desc
                ));
            }
        }
    }

    // Request body
    if let Some(body) = op.and_then(|o| o.get("requestBody")) {
        md.push_str("\n### Request Body\n\n");
        if let Some(desc) = body.get("description").and_then(|v| v.as_str()) {
            md.push_str(desc);
            md.push('\n');
        }
        if let Some(content) = body.get("content").and_then(|v| v.as_object()) {
            for (content_type, media) in content {
                md.push_str(&format!("- Content-Type: {}\n", content_type));
                if let Some(schema_val) = media.get("schema") {
                    let summary = format_schema_summary(schema_val, schemas);
                    md.push_str(&format!("  Schema: {}\n", summary));
                }
            }
        }
    }

    // Responses table
    if let Some(responses) = op
        .and_then(|o| o.get("responses"))
        .and_then(|v| v.as_object())
    {
        if !responses.is_empty() {
            md.push_str("\n### Responses\n\n");
            md.push_str("| Status | Description |\n");
            md.push_str("|--------|-------------|\n");
            for (status, resp) in responses {
                let desc = resp
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-");
                md.push_str(&format!("| {} | {} |\n", status, desc));
            }
        }
    }

    md
}

// ---------------------------------------------------------------------------
// $ref resolution
// ---------------------------------------------------------------------------

/// Resolve a `$ref` string and produce a human-readable summary.
fn resolve_ref(ref_path: &str, schemas: &HashMap<String, &Value>) -> String {
    let prefix = "#/components/schemas/";
    if let Some(schema_name) = ref_path.strip_prefix(prefix) {
        if let Some(schema) = schemas.get(schema_name) {
            return format_schema_summary(schema, schemas);
        }
        return format!("(未找到引用: {})", ref_path);
    }
    // Non-local refs are ignored
    String::new()
}

/// Format a JSON Schema value into a compact inline summary.
fn format_schema_summary(schema: &Value, schemas: &HashMap<String, &Value>) -> String {
    // Handle $ref
    if let Some(ref_path) = schema.get("$ref").and_then(|v| v.as_str()) {
        return resolve_ref(ref_path, schemas);
    }

    let obj = match schema.as_object() {
        Some(o) => o,
        None => return String::new(),
    };

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("object");

    match typ {
        "object" => {
            let props = obj.get("properties").and_then(|v| v.as_object());
            let required_fields: Vec<&str> = obj
                .get("required")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            if let Some(props) = props {
                let fields: Vec<String> = props
                    .iter()
                    .map(|(name, prop)| {
                        let prop_type = prop
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let is_required = required_fields.contains(&name.as_str());
                        if is_required {
                            format!("{}: {} (required)", name, prop_type)
                        } else {
                            format!("{}: {}", name, prop_type)
                        }
                    })
                    .collect();
                format!("{{ {} }}", fields.join(", "))
            } else {
                "object".to_string()
            }
        }
        "array" => {
            if let Some(items) = obj.get("items") {
                let item_summary = format_schema_summary(items, schemas);
                format!("array<{}>", item_summary)
            } else {
                "array".to_string()
            }
        }
        other => other.to_string(),
    }
}
