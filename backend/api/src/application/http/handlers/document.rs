use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    Json,
};
use rwiki_core::domain::document::{DocumentRow, DocumentStatus};
use rwiki_core::infrastructure::document_chunk::DocumentChunk;
use rwiki_core::infrastructure::faq_parser;
use rwiki_core::infrastructure::markdown_parser;
use rwiki_core::infrastructure::openapi_parser;
use rwiki_core::infrastructure::text_chunker;
use rwiki_core::infrastructure::vector_store::IndexOptions;
use rwiki_core::infrastructure::xlsx_parser;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::application::http::errors::{ApiError, ErrorResponse};
use crate::application::http::state::AppState;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadDocumentResponse {
    pub id: Uuid,
    pub file_name: String,
    pub status: String,
    pub row_count: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentListItem {
    pub id: Uuid,
    pub file_name: String,
    pub status: String,
    pub row_count: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DocumentListResponse {
    pub documents: Vec<DocumentListItem>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PublishDocumentResponse {
    pub id: Uuid,
    pub status: DocumentStatus,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Upload and index a document (.xlsx / .md / .mdx / .json / .jsonl).
///
/// Accepts a multipart file upload, validates format and size (<= 50 MB),
/// parses into chunks, embeds them via VectorStoreManager, and persists
/// metadata to SQLite.
#[utoipa::path(
    post,
    path = "/api/documents/upload",
    tag = "documents",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Document uploaded successfully as draft", body = UploadDocumentResponse),
        (status = 400, description = "Invalid file (unsupported format, empty, encoding error, or frontmatter error)", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 413, description = "File exceeds 50MB limit", body = ErrorResponse),
        (status = 500, description = "Processing failed", body = ErrorResponse)
    )
)]
pub async fn upload_document(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadDocumentResponse>, ApiError> {
    // Extract the `file` and optional `refresh_embed` fields from multipart
    let mut refresh_embed = false;
    let mut file_name = None;
    let mut file_bytes = None;

    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|e| ApiError::bad_request(format!("无法读取上传字段: {e}")))?;

        let field = match field {
            Some(f) => f,
            None => break,
        };

        match field.name() {
            Some("refresh_embed") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::bad_request(format!("读取 refresh_embed 失败: {e}")))?;
                refresh_embed = text == "true";
            }
            Some("file") => {
                let name = field.file_name().unwrap_or("unknown.xlsx").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiError::bad_request(format!("读取文件内容失败: {e}")))?;
                file_name = Some(name);
                file_bytes = Some(bytes);
            }
            _ => {}
        }
    }

    let file_name = file_name.ok_or_else(|| ApiError::bad_request("缺少 file 字段"))?;
    let bytes = file_bytes.ok_or_else(|| ApiError::bad_request("缺少 file 字段"))?;

    // Validate extension
    let ext = file_name.to_lowercase();
    let ext = if ext.ends_with(".xlsx") {
        "xlsx"
    } else if ext.ends_with(".md") {
        "md"
    } else if ext.ends_with(".mdx") {
        "mdx"
    } else if ext.ends_with(".json") {
        "json"
    } else if ext.ends_with(".jsonl") {
        "jsonl"
    } else {
        return Err(ApiError::bad_request(
            "不支持的文件格式，支持 xlsx/md/mdx/json/jsonl",
        ));
    };

    // Validate size (50 MB)
    const MAX_SIZE: usize = 50 * 1024 * 1024;
    if bytes.len() > MAX_SIZE {
        return Err(ApiError::payload_too_large("文件大小超过 50MB 限制"));
    }

    // Route by file format
    let (parsed_chunks, row_count) = match ext {
        "xlsx" => {
            // Validate file magic bytes (ZIP header = PK\x03\x04)
            const XLSX_MAGIC: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
            if bytes.len() < 4 || bytes[..4] != XLSX_MAGIC {
                return Err(ApiError::bad_request(
                    "文件内容不是有效的 xlsx 格式（缺少 ZIP 文件头）",
                ));
            }

            // Parse xlsx (Wiki-style structured format)
            let parse_result = xlsx_parser::parse_xlsx_wiki(&bytes)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;

            let row_count = parse_result.pages.len() as i32;

            // Convert WikiPage results to ParsedChunk for chunking pipeline
            let parsed_chunks: Vec<_> = parse_result
                .pages
                .into_iter()
                .map(|page| xlsx_parser::ParsedChunk {
                    content: page.markdown,
                    page_id: page.page_id,
                    sub_index: None,
                    title: page.title,
                    locale: page.locale,
                    link: page.link,
                    tags: page.tags,
                    section: None,
                    ..Default::default()
                })
                .collect();

            (parsed_chunks, row_count)
        }
        "md" | "mdx" => {
            let chunk = markdown_parser::parse_markdown_file(&bytes, &file_name)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            (vec![chunk], 1)
        }
        "json" => {
            // `.json` exclusively routes to the OpenAPI parser; FAQ is now
            // handled by `.jsonl`.
            let chunks = openapi_parser::parse_openapi_file(&bytes, &file_name)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            let row_count = chunks.len() as i32;
            (chunks, row_count)
        }
        "jsonl" => {
            let chunks = faq_parser::parse_faq_file(&bytes)
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            let row_count = chunks.len() as i32;
            (chunks, row_count)
        }
        _ => unreachable!(),
    };

    let document_id = Uuid::now_v7();

    // Split long chunks that exceed embedding model token limits
    let parsed_chunks = text_chunker::split_long_chunks_with_section_default(parsed_chunks);

    // Insert document record with status `processing`
    let doc_id_str = document_id.to_string();
    let file_name_clone = file_name.clone();
    state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count) VALUES (?, ?, 'processing', ?)",
                rusqlite::params![doc_id_str, file_name_clone, row_count],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .map_err(|e| ApiError::internal(format!("数据库写入失败: {e}")))?;

    // Convert parsed chunks to embeddable DocumentChunks
    let chunks: Vec<DocumentChunk> = parsed_chunks
        .iter()
        .map(|pc| DocumentChunk::from_parsed(document_id, pc))
        .collect();

    // Index into vector store
    match state
        .vector_store
        .index_document_with_options(document_id, chunks, IndexOptions { refresh_embed })
        .await
    {
        Ok(_) => {
            // Update status to `draft`
            let doc_id_str = document_id.to_string();
            state
                .sqlite
                .call(move |conn| {
                    conn.execute(
                        "UPDATE documents SET status = 'draft' WHERE id = ?",
                        rusqlite::params![doc_id_str],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .map_err(|e| ApiError::internal(format!("更新文档状态失败: {e}")))?;

            let doc_id_str = document_id.to_string();
            let created_at: String = state
                .sqlite
                .call(move |conn| {
                    let created_at: String = conn.query_row(
                        "SELECT created_at FROM documents WHERE id = ?",
                        rusqlite::params![doc_id_str],
                        |row| row.get(0),
                    )?;
                    Ok::<String, rusqlite::Error>(created_at)
                })
                .await
                .map_err(|e| ApiError::internal(format!("查询文档失败: {e}")))?;

            Ok(Json(UploadDocumentResponse {
                id: document_id,
                file_name,
                status: "draft".to_string(),
                row_count,
                created_at,
            }))
        }
        Err(e) => {
            // Update status to `failed`
            let err_msg = e.to_string();
            let doc_id_str = document_id.to_string();
            let _ = state
                .sqlite
                .call(move |conn| {
                    conn.execute(
                        "UPDATE documents SET status = 'failed', error_message = ? WHERE id = ?",
                        rusqlite::params![err_msg, doc_id_str],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                })
                .await;

            Err(ApiError::internal("文件处理失败，请稍后重试"))
        }
    }
}

/// List all uploaded documents.
#[utoipa::path(
    get,
    path = "/api/documents",
    tag = "documents",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Document list", body = DocumentListResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse)
    )
)]
pub async fn list_documents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DocumentListResponse>, ApiError> {
    let rows: Vec<DocumentRow> = state
        .sqlite
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, file_name, status, row_count, error_message, created_at FROM documents ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(DocumentRow {
                        id: row.get(0)?,
                        file_name: row.get(1)?,
                        status: row.get(2)?,
                        row_count: row.get(3)?,
                        error_message: row.get(4)?,
                        created_at: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<Vec<DocumentRow>, rusqlite::Error>(rows)
        })
        .await
        .map_err(|e| ApiError::internal(format!("获取文档列表失败: {e}")))?;

    let documents: Result<Vec<DocumentListItem>, ApiError> = rows
        .into_iter()
        .map(|row| {
            Ok(DocumentListItem {
                id: Uuid::parse_str(&row.id)
                    .map_err(|e| ApiError::internal(format!("文档ID格式错误: {e}")))?,
                file_name: row.file_name,
                status: row.status,
                row_count: row.row_count,
                error_message: row.error_message,
                created_at: row.created_at,
            })
        })
        .collect();
    let documents = documents?;

    Ok(Json(DocumentListResponse { documents }))
}

/// Delete a document by ID.
#[utoipa::path(
    delete,
    path = "/api/documents/{documentId}",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID")
    ),
    responses(
        (status = 204, description = "Document deleted"),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 500, description = "Vector store removal failed", body = ErrorResponse)
    )
)]
pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let doc_id_str = document_id.to_string();

    // Verify the document exists first
    let exists: bool = state
        .sqlite
        .call(move |conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ?)",
                rusqlite::params![doc_id_str],
                |row| row.get(0),
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("查询文档失败: {e}")))?;

    if !exists {
        return Err(ApiError::not_found("文档不存在"));
    }

    // Remove from vector store BEFORE DB delete to avoid orphan vectors on failure.
    // remove_document is idempotent: Ok if no chunks exist (document may not have been indexed).
    if let Err(e) = state.vector_store.remove_document(&document_id).await {
        tracing::error!(%document_id, "向量存储删除失败: {e}");
        return Err(ApiError::internal("向量存储删除失败，请稍后重试"));
    }

    // Now safe to delete from DB
    let doc_id_str = document_id.to_string();
    state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "DELETE FROM documents WHERE id = ?",
                rusqlite::params![doc_id_str],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .map_err(|e| ApiError::internal(format!("删除文档失败: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Publish a draft document.
///
/// Changes document status from "draft" to "published", making its content
/// available for RAG search. Only draft documents can be published.
#[utoipa::path(
    patch,
    path = "/api/documents/{documentId}/publish",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID")
    ),
    responses(
        (status = 200, description = "Document published", body = PublishDocumentResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Only draft documents can be published", body = ErrorResponse)
    )
)]
pub async fn publish_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<PublishDocumentResponse>, ApiError> {
    let doc_id_str = document_id.to_string();
    let rows_affected = state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ? AND status = 'draft'",
                rusqlite::params![doc_id_str],
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("更新文档状态失败: {e}")))?;

    if rows_affected == 0 {
        let doc_id_str = document_id.to_string();
        let exists: bool = state
            .sqlite
            .call(move |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ?)",
                    rusqlite::params![doc_id_str],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(|e| ApiError::internal(format!("查询文档失败: {e}")))?;

        if !exists {
            return Err(ApiError::not_found("文档不存在"));
        }
        return Err(ApiError::conflict("仅草稿状态的文档可以发布"));
    }

    Ok(Json(PublishDocumentResponse {
        id: document_id,
        status: DocumentStatus::Published,
    }))
}

/// Unpublish a published document.
///
/// Changes document status from "published" back to "draft", removing its
/// content from RAG search results. Only published documents can be unpublished.
#[utoipa::path(
    patch,
    path = "/api/documents/{documentId}/unpublish",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID")
    ),
    responses(
        (status = 200, description = "Document unpublished", body = PublishDocumentResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Only published documents can be unpublished", body = ErrorResponse)
    )
)]
pub async fn unpublish_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
) -> Result<Json<PublishDocumentResponse>, ApiError> {
    let doc_id_str = document_id.to_string();
    let rows_affected = state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'draft' WHERE id = ? AND status = 'published'",
                rusqlite::params![doc_id_str],
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("更新文档状态失败: {e}")))?;

    if rows_affected == 0 {
        let doc_id_str = document_id.to_string();
        let exists: bool = state
            .sqlite
            .call(move |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ?)",
                    rusqlite::params![doc_id_str],
                    |row| row.get(0),
                )
            })
            .await
            .map_err(|e| ApiError::internal(format!("查询文档失败: {e}")))?;

        if !exists {
            return Err(ApiError::not_found("文档不存在"));
        }
        return Err(ApiError::conflict("仅已发布的文档可以取消发布"));
    }

    Ok(Json(PublishDocumentResponse {
        id: document_id,
        status: DocumentStatus::Draft,
    }))
}
