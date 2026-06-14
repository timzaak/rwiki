//! Scenario tests for OpenAPI 3.x JSON parsing logic.
//!
//! Tests cover valid parsing, error cases, title priority, tag/section
//! extraction, $ref resolution, and formatting of parameters/responses.

use super::openapi_parser::{parse_openapi_file, OpenApiParseError};

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Build a minimal valid OpenAPI 3.x JSON with one GET endpoint.
fn minimal_openapi_json() -> String {
    r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "summary": "List users",
                    "responses": {
                        "200": { "description": "OK" }
                    }
                }
            }
        }
    }"#
    .to_string()
}

// ---------------------------------------------------------------------------
// Scenario tests
// ---------------------------------------------------------------------------

#[test]
fn valid_openapi_3x_produces_endpoint_chunks() {
    let json = minimal_openapi_json();
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    assert_eq!(result.len(), 1, "one endpoint = one chunk");
    let chunk = &result[0];
    assert!(
        chunk.content.contains("GET /users"),
        "content should contain method and path"
    );
    assert_eq!(chunk.title, "List users");
    assert!(
        !chunk.page_id.to_string().is_empty(),
        "page_id should be non-empty"
    );
}

#[test]
fn invalid_json_syntax_returns_error() {
    let bad_json = b"{ invalid json }";
    let result = parse_openapi_file(bad_json, "bad.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::InvalidJson(msg) => {
            assert!(
                !msg.is_empty(),
                "error message should describe the JSON issue"
            );
        }
        other => panic!("expected InvalidJson, got: {:?}", other),
    }
}

#[test]
fn missing_openapi_field_returns_error() {
    let json = r#"{
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": { "/users": { "get": {} } }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "no-openapi.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::MissingOpenApiField => {}
        other => panic!("expected MissingOpenApiField, got: {:?}", other),
    }
}

#[test]
fn unsupported_version_returns_error() {
    let json = r#"{
        "openapi": "2.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": { "/users": { "get": {} } }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "swagger.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::UnsupportedVersion => {}
        other => panic!("expected UnsupportedVersion, got: {:?}", other),
    }
}

#[test]
fn empty_paths_returns_error() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {}
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "empty.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::EmptyPaths => {}
        other => panic!("expected EmptyPaths, got: {:?}", other),
    }
}

#[test]
fn missing_paths_returns_error() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "no-paths.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::EmptyPaths => {}
        other => panic!("expected EmptyPaths, got: {:?}", other),
    }
}

#[test]
fn title_prefers_operation_id() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "summary": "List all users",
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    assert_eq!(
        result[0].title, "listUsers",
        "operationId should take priority"
    );
}

#[test]
fn title_falls_back_to_summary() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "summary": "List all users",
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    assert_eq!(
        result[0].title, "List all users",
        "summary is used when no operationId"
    );
}

#[test]
fn title_falls_back_to_path() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    assert_eq!(
        result[0].title, "/users",
        "path is used when no operationId or summary"
    );
}

#[test]
fn section_from_first_tag() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "tags": ["users", "admin"],
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    assert_eq!(result[0].tags, vec!["users", "admin"]);
    assert_eq!(
        result[0].section.as_deref(),
        Some("users"),
        "section = first tag"
    );
}

#[test]
fn ref_resolution_inlines_schema() {
    let json = r##"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/User" }
                            }
                        }
                    },
                    "responses": { "200": { "description": "OK" } }
                }
            }
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "integer" },
                        "name": { "type": "string" }
                    },
                    "required": ["id", "name"]
                }
            }
        }
    }"##;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    let content = &result[0].content;
    assert!(
        content.contains("id: integer (required)"),
        "should inline resolved schema with required field, got: {}",
        content
    );
    assert!(
        content.contains("name: string (required)"),
        "should inline resolved schema with required field, got: {}",
        content
    );
}

#[test]
fn ref_not_found_produces_note() {
    let json = r##"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/Missing" }
                            }
                        }
                    },
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"##;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    let content = &result[0].content;
    assert!(
        content.contains("unresolved $ref"),
        "should note that ref was not found, got: {}",
        content
    );
}

#[test]
fn non_utf8_returns_encoding_error() {
    let bad_bytes: &[u8] = &[0xFF, 0xFE, 0x00, 0x01];
    let result = parse_openapi_file(bad_bytes, "bad.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::InvalidEncoding => {}
        other => panic!("expected InvalidEncoding, got: {:?}", other),
    }
}

#[test]
fn parameters_table_included() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users/{id}": {
                "get": {
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" },
                            "description": "User ID"
                        }
                    ],
                    "responses": { "200": { "description": "OK" } }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    let content = &result[0].content;
    assert!(
        content.contains("### Parameters"),
        "should have Parameters heading"
    );
    assert!(
        content.contains("| id | path |"),
        "should contain parameter row"
    );
    assert!(content.contains("|------|"), "should have table separator");
}

#[test]
fn responses_table_included() {
    let json = r#"{
        "openapi": "3.0.0",
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": {
            "/users": {
                "get": {
                    "responses": {
                        "200": { "description": "List of users" },
                        "401": { "description": "Unauthorized" }
                    }
                }
            }
        }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json").expect("should parse");

    let content = &result[0].content;
    assert!(
        content.contains("### Responses"),
        "should have Responses heading"
    );
    assert!(
        content.contains("| 200 | List of users |"),
        "should contain 200 response row"
    );
    assert!(
        content.contains("| 401 | Unauthorized |"),
        "should contain 401 response row"
    );
}

#[test]
fn non_string_openapi_field_returns_missing_field() {
    let json = r#"{
        "openapi": 3,
        "info": { "title": "Test", "version": "1.0.0" },
        "paths": { "/users": { "get": { "responses": { "200": { "description": "OK" } } } } }
    }"#;
    let result = parse_openapi_file(json.as_bytes(), "api.json");

    assert!(result.is_err());
    match result.unwrap_err() {
        OpenApiParseError::MissingOpenApiField => {}
        other => panic!(
            "expected MissingOpenApiField for non-string openapi field, got: {:?}",
            other
        ),
    }
}
