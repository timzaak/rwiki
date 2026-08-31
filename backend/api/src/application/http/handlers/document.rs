use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use rwiki_core::config::ChannelValidationError;
use rwiki_core::domain::document::{DocumentRow, DocumentStatus};
use rwiki_core::infrastructure::document_chunk::DocumentChunk;
use rwiki_core::infrastructure::faq_parser;
use rwiki_core::infrastructure::markdown_parser;
use rwiki_core::infrastructure::openapi_parser;
use rwiki_core::infrastructure::text_chunker;
use rwiki_core::infrastructure::vector_store::IndexOptions;
use rwiki_core::infrastructure::xlsx_parser;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
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
    pub channel_id: String,
    pub created_at: String,
}

/// multipart/form-data body for document upload.
///
/// NOTE: this struct exists only for OpenAPI schema generation (utoipa); the
/// handler does not deserialize it. Actual multipart field parsing lives in
/// `upload_document` (reads fields by name "file" / "channelId" / "refresh_embed",
/// where `refresh_embed` is judged by `text == "true"`, slightly diverging from
/// Option<bool> semantics). When adding or renaming fields, this schema and the
/// handler parsing must be kept in sync, otherwise generated clients and the
/// server contract will silently drift.
#[derive(utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadDocumentRequest {
    /// The uploaded file (binary).
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
    /// Whether to re-embed if the content already exists. Optional, defaults false.
    pub refresh_embed: Option<bool>,
    /// The channel this document belongs to. Required.
    pub channel_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentListItem {
    pub id: Uuid,
    pub file_name: String,
    pub status: String,
    pub row_count: i32,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub created_at: String,
}

/// Query parameters for channel-scoped document lifecycle operations.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct DocumentChannelQuery {
    /// The channel ID that owns the document.
    pub channel_id: String,
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

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchStatusRequest {
    pub publish: Vec<Uuid>,
    pub unpublish: Vec<Uuid>,
    /// The channel ID that owns the documents.
    pub channel_id: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum BatchAction {
    Publish,
    Unpublish,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchStatusItem {
    pub document_id: Uuid,
    pub action: BatchAction,
    pub applied: bool,
    pub status: String,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchStatusResponse {
    pub results: Vec<BatchStatusItem>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Upload and index a document (.xlsx / .md / .mdx / .json / .jsonl).
///
/// Accepts a multipart file upload, validates format and size (<= 50 MB),
/// parses into chunks, embeds them via VectorStoreManager, and persists
/// metadata to SQLite. The document is associated with the provided `channelId`.
#[utoipa::path(
    post,
    path = "/api/documents/upload",
    tag = "documents",
    security(("bearer_auth" = [])),
    request_body(content = inline(UploadDocumentRequest), content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "Document uploaded successfully as draft", body = UploadDocumentResponse),
        (status = 400, description = "Invalid file or missing/invalid channelId (unsupported format, empty, encoding error, or frontmatter error)", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 413, description = "File exceeds 50MB limit", body = ErrorResponse),
        (status = 500, description = "Processing failed", body = ErrorResponse)
    )
)]
pub async fn upload_document(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadDocumentResponse>, ApiError> {
    // Extract the `file`, optional `refresh_embed`, and required `channelId` fields from multipart
    let mut refresh_embed = false;
    let mut file_name = None;
    let mut file_bytes = None;
    let mut channel_id = None;

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
            Some("channelId") => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| ApiError::bad_request(format!("读取 channelId 失败: {e}")))?;
                channel_id = Some(text);
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

    let channel_id = channel_id.ok_or_else(|| ApiError::bad_request("缺少 channelId 字段"))?;
    let channel_id = state
        .channels_config
        .require_configured(&channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

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
    let channel_id_for_insert = channel_id.clone();
    state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "INSERT INTO documents (id, file_name, status, row_count, channel_id) VALUES (?, ?, 'processing', ?, ?)",
                rusqlite::params![doc_id_str, file_name_clone, row_count, channel_id_for_insert],
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
        Ok(stats) => {
            // 向量复用 reporting：记录复用缓存向量的 chunk 数（Rule 12 fail loud）
            if stats.reused > 0 {
                tracing::info!(
                    %document_id,
                    indexed = stats.indexed,
                    reused = stats.reused,
                    "上传文档索引完成（复用缓存向量）"
                );
            } else {
                tracing::info!(%document_id, indexed = stats.indexed, "上传文档索引完成");
            }
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
                channel_id,
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

/// List uploaded documents for a specific channel.
#[utoipa::path(
    get,
    path = "/api/documents",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(DocumentChannelQuery),
    responses(
        (status = 200, description = "Document list", body = DocumentListResponse),
        (status = 400, description = "Missing or invalid channelId", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse)
    )
)]
pub async fn list_documents(
    State(state): State<Arc<AppState>>,
    Query(query): Query<DocumentChannelQuery>,
) -> Result<Json<DocumentListResponse>, ApiError> {
    let channel_id = state
        .channels_config
        .require_configured(&query.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    let rows: Vec<DocumentRow> = state
        .sqlite
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, file_name, status, row_count, channel_id, error_message, created_at FROM documents WHERE channel_id = ? ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![channel_id], |row| {
                    Ok(DocumentRow {
                        id: row.get(0)?,
                        file_name: row.get(1)?,
                        status: row.get(2)?,
                        row_count: row.get(3)?,
                        channel_id: row.get(4)?,
                        error_message: row.get(5)?,
                        created_at: row.get(6)?,
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
                channel_id: row.channel_id,
                error_message: row.error_message,
                created_at: row.created_at,
            })
        })
        .collect();
    let documents = documents?;

    Ok(Json(DocumentListResponse { documents }))
}

/// Delete a document by ID within a channel.
#[utoipa::path(
    delete,
    path = "/api/documents/{documentId}",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID"),
        DocumentChannelQuery
    ),
    responses(
        (status = 204, description = "Document deleted"),
        (status = 400, description = "Missing or invalid channelId", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 500, description = "Vector store removal failed", body = ErrorResponse)
    )
)]
pub async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
    Query(query): Query<DocumentChannelQuery>,
) -> Result<StatusCode, ApiError> {
    let channel_id = state
        .channels_config
        .require_configured(&query.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    let doc_id_str = document_id.to_string();

    // Verify the document exists and belongs to the requested channel
    let exists: bool = state
        .sqlite
        .call(move |conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ? AND channel_id = ?)",
                rusqlite::params![doc_id_str, channel_id],
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
    let channel_id = query.channel_id.trim().to_string();
    state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "DELETE FROM documents WHERE id = ? AND channel_id = ?",
                rusqlite::params![doc_id_str, channel_id],
            )?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .map_err(|e| ApiError::internal(format!("删除文档失败: {e}")))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Publish a draft document within a channel.
///
/// Changes document status from "draft" to "published", making its content
/// available for RAG search. Only draft documents belonging to the requested
/// channel can be published.
#[utoipa::path(
    patch,
    path = "/api/documents/{documentId}/publish",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID"),
        DocumentChannelQuery
    ),
    responses(
        (status = 200, description = "Document published", body = PublishDocumentResponse),
        (status = 400, description = "Missing or invalid channelId", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Only draft documents can be published", body = ErrorResponse)
    )
)]
pub async fn publish_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
    Query(query): Query<DocumentChannelQuery>,
) -> Result<Json<PublishDocumentResponse>, ApiError> {
    let channel_id = state
        .channels_config
        .require_configured(&query.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    let doc_id_str = document_id.to_string();
    let rows_affected = state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'published' WHERE id = ? AND channel_id = ? AND status = 'draft'",
                rusqlite::params![doc_id_str, channel_id],
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("更新文档状态失败: {e}")))?;

    if rows_affected == 0 {
        let doc_id_str = document_id.to_string();
        let channel_id = query.channel_id.trim().to_string();
        let exists: bool = state
            .sqlite
            .call(move |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ? AND channel_id = ?)",
                    rusqlite::params![doc_id_str, channel_id],
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

/// Unpublish a published document within a channel.
///
/// Changes document status from "published" back to "draft", removing its
/// content from RAG search results. Only published documents belonging to the
/// requested channel can be unpublished.
#[utoipa::path(
    patch,
    path = "/api/documents/{documentId}/unpublish",
    tag = "documents",
    security(("bearer_auth" = [])),
    params(
        ("documentId" = Uuid, Path, description = "Document ID"),
        DocumentChannelQuery
    ),
    responses(
        (status = 200, description = "Document unpublished", body = PublishDocumentResponse),
        (status = 400, description = "Missing or invalid channelId", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 404, description = "Document not found", body = ErrorResponse),
        (status = 409, description = "Only published documents can be unpublished", body = ErrorResponse)
    )
)]
pub async fn unpublish_document(
    State(state): State<Arc<AppState>>,
    Path(document_id): Path<Uuid>,
    Query(query): Query<DocumentChannelQuery>,
) -> Result<Json<PublishDocumentResponse>, ApiError> {
    let channel_id = state
        .channels_config
        .require_configured(&query.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    let doc_id_str = document_id.to_string();
    let rows_affected = state
        .sqlite
        .call(move |conn| {
            conn.execute(
                "UPDATE documents SET status = 'draft' WHERE id = ? AND channel_id = ? AND status = 'published'",
                rusqlite::params![doc_id_str, channel_id],
            )
        })
        .await
        .map_err(|e| ApiError::internal(format!("更新文档状态失败: {e}")))?;

    if rows_affected == 0 {
        let doc_id_str = document_id.to_string();
        let channel_id = query.channel_id.trim().to_string();
        let exists: bool = state
            .sqlite
            .call(move |conn| {
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM documents WHERE id = ? AND channel_id = ?)",
                    rusqlite::params![doc_id_str, channel_id],
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

/// Batch update document status (publish/unpublish multiple documents) within a channel.
///
/// Atomically updates multiple document statuses in a single transaction.
/// Only valid status transitions are applied (draft→published, published→draft)
/// for documents belonging to the requested channel.
/// Invalid transitions or missing documents are reported in the response without blocking valid operations.
#[utoipa::path(
    post,
    path = "/api/documents/batch-status",
    tag = "documents",
    security(("bearer_auth" = [])),
    request_body = BatchStatusRequest,
    responses(
        (status = 200, description = "Batch status update completed", body = BatchStatusResponse),
        (status = 400, description = "Missing/invalid channelId or both arrays empty", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse)
    )
)]
pub async fn batch_update_status(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BatchStatusRequest>,
) -> Result<Json<BatchStatusResponse>, ApiError> {
    // Validate channel_id before the empty-arrays check so an unknown channel
    // surfaces instead of the misleading "publish 和 unpublish 不能同时为空".
    let channel_id = state
        .channels_config
        .require_configured(&req.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    // Validate at least one collection is non-empty
    if req.publish.is_empty() && req.unpublish.is_empty() {
        return Err(ApiError::bad_request("publish 和 unpublish 不能同时为空"));
    }

    // 去重:同一列表内重复 UUID 会产生重复结果项与重复 IN 参数。
    // publish 与 unpublish 各自独立去重,保留首次出现顺序;
    // 同时存在于两个列表的 ID 是合法输入(由状态守卫处理),保持两条结果项。
    let publish_ids = dedupe_preserve_order(req.publish);
    let unpublish_ids = dedupe_preserve_order(req.unpublish);

    let all_ids: Vec<Uuid> = publish_ids
        .iter()
        .chain(unpublish_ids.iter())
        .copied()
        .collect();

    let results = state
        .sqlite
        .call(move |conn| {
            let tx = conn.transaction()?;

            // 1. Fetch current status for all requested IDs belonging to the channel
            let mut current_statuses = HashMap::new();
            if !all_ids.is_empty() {
                // Build parameterized query with proper rusqlite params handling
                let statuses: Vec<(String, String)> = if all_ids.len() <= 900 {
                    // rusqlite supports up to ~900 parameters, use direct IN clause
                    let placeholders = (0..all_ids.len())
                        .map(|_| "?")
                        .collect::<Vec<_>>()
                        .join(",");
                    let sql = format!(
                        "SELECT id, status FROM documents WHERE channel_id = ? AND id IN ({})",
                        placeholders
                    );

                    let ids_as_strings: Vec<String> =
                        all_ids.iter().map(|u| u.to_string()).collect();
                    let params: Vec<Box<dyn rusqlite::ToSql>> = std::iter::once(
                        Box::new(channel_id.clone()) as Box<dyn rusqlite::ToSql>,
                    )
                    .chain(
                        ids_as_strings
                            .iter()
                            .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>),
                    )
                    .collect();

                    // 占位符数量随批次大小变化,prepare_cached 按 SQL 文本做缓存键,
                    // 此处永远命中不到缓存且会持续泄漏条目,改用 prepare。
                    let mut stmt = tx.prepare(&sql)?;
                    let rows =
                        stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                            let id: String = row.get(0)?;
                            let status: String = row.get(1)?;
                            Ok((id, status))
                        })?;

                    rows.filter_map(|r| r.ok()).collect()
                } else {
                    // For very large batches, use iterative approach (unlikely in practice)
                    all_ids
                        .iter()
                        .filter_map(|id| {
                            tx.query_row(
                                "SELECT id, status FROM documents WHERE channel_id = ? AND id = ?",
                                rusqlite::params![channel_id.clone(), id.to_string()],
                                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                            )
                            .ok()
                        })
                        .collect()
                };

                for (id_str, status) in statuses {
                    if let Ok(uuid) = Uuid::parse_str(&id_str) {
                        current_statuses.insert(uuid, status);
                    }
                }
            }

            // 2. Build results array with individual guards
            let mut response_items = Vec::new();
            let mut valid_publish_ids = Vec::new();
            let mut valid_unpublish_ids = Vec::new();

            for id in &publish_ids {
                let (applied, final_status, reason) = match current_statuses.get(id) {
                    None => (
                        false,
                        "not_found".to_string(),
                        Some("not_found".to_string()),
                    ),
                    Some(status) if status == "draft" => {
                        valid_publish_ids.push(*id);
                        (true, "published".to_string(), None)
                    }
                    Some(status) => (false, status.clone(), Some("invalid_status".to_string())),
                };
                response_items.push(BatchStatusItem {
                    document_id: *id,
                    action: BatchAction::Publish,
                    applied,
                    status: final_status,
                    reason,
                });
            }

            for id in &unpublish_ids {
                let (applied, final_status, reason) = match current_statuses.get(id) {
                    None => (
                        false,
                        "not_found".to_string(),
                        Some("not_found".to_string()),
                    ),
                    Some(status) if status == "published" => {
                        valid_unpublish_ids.push(*id);
                        (true, "draft".to_string(), None)
                    }
                    Some(status) => (false, status.clone(), Some("invalid_status".to_string())),
                };
                response_items.push(BatchStatusItem {
                    document_id: *id,
                    action: BatchAction::Unpublish,
                    applied,
                    status: final_status,
                    reason,
                });
            }

            // 3. Execute batched updates with proper parameterization.
            // SQLite 默认 SQLITE_MAX_VARIABLE_NUMBER=999,单臂 >999 个合法 ID 会让
            // IN 子句参数超限并回滚整个事务。按 900 分块,每块仍保留状态守卫。
            const UPDATE_CHUNK: usize = 900;

            for chunk in valid_publish_ids.chunks(UPDATE_CHUNK) {
                let ids: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
                let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "UPDATE documents SET status='published' WHERE channel_id = ? AND id IN ({}) AND status='draft'",
                    placeholders
                );
                let params: Vec<Box<dyn rusqlite::ToSql>> = std::iter::once(
                    Box::new(channel_id.clone()) as Box<dyn rusqlite::ToSql>,
                )
                .chain(ids.into_iter().map(|s| Box::new(s) as Box<dyn rusqlite::ToSql>))
                .collect();
                tx.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
            }

            for chunk in valid_unpublish_ids.chunks(UPDATE_CHUNK) {
                let ids: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
                let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
                let sql = format!(
                    "UPDATE documents SET status='draft' WHERE channel_id = ? AND id IN ({}) AND status='published'",
                    placeholders
                );
                let params: Vec<Box<dyn rusqlite::ToSql>> = std::iter::once(
                    Box::new(channel_id.clone()) as Box<dyn rusqlite::ToSql>,
                )
                .chain(ids.into_iter().map(|s| Box::new(s) as Box<dyn rusqlite::ToSql>))
                .collect();
                tx.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
            }

            tx.commit()?;

            Ok::<BatchStatusResponse, rusqlite::Error>(BatchStatusResponse {
                results: response_items,
            })
        })
        .await
        .map_err(|e| ApiError::internal(format!("批量状态更新失败: {}", e)))?;

    Ok(Json(results))
}

/// 去重并保留首次出现顺序。
fn dedupe_preserve_order(ids: Vec<Uuid>) -> Vec<Uuid> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if seen.insert(id) {
            out.push(id);
        }
    }
    out
}
