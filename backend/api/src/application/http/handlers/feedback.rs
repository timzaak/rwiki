use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

use crate::application::http::errors::ApiError;
use crate::application::http::state::AppState;
use rwiki_core::config::ChannelValidationError;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRequest {
    /// 频道标识数组；必填（在 handler 中二次校验以返回 400）。支持单频道或多频道。
    #[serde(default)]
    pub channel_id: Option<Vec<String>>,
    pub session_id: String,
    pub message_id: String,
    pub feedback: Option<String>,
    pub user_message: String,
    pub assistant_message: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackItem {
    pub id: i64,
    /// 反馈所属频道
    pub channel_id: String,
    pub session_id: String,
    pub message_id: String,
    pub feedback: String,
    pub user_message: String,
    pub assistant_message: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct FeedbackListResponse {
    pub items: Vec<FeedbackItem>,
    pub total: i64,
}

#[derive(Debug, Deserialize, IntoParams, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackQueryParams {
    /// 频道标识；必填
    pub channel_id: String,
    pub feedback: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Submit or cancel chat feedback.
///
/// When `feedback` is "like" or "dislike", performs an UPSERT.
/// When `feedback` is null, deletes the existing record (idempotent).
#[utoipa::path(
    post,
    path = "/api/chat/feedback",
    tag = "chat",
    request_body = FeedbackRequest,
    responses(
        (status = 204, description = "Feedback submitted or cancelled"),
        (status = 400, description = "Invalid request", body = crate::application::http::errors::ErrorResponse),
        (status = 500, description = "Database error", body = crate::application::http::errors::ErrorResponse)
    )
)]
pub async fn submit_feedback(
    State(state): State<Arc<AppState>>,
    Json(req): Json<FeedbackRequest>,
) -> Result<StatusCode, ApiError> {
    // 批量校验频道；多频道时取排序后首个频道写入 chat_feedback.channel_id（单 TEXT 列），
    // 保持 list 查询单频道筛选语义不变。
    let channel_ids = state
        .channels_config
        .require_all_configured(req.channel_id.as_deref().unwrap_or(&[]))
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?;
    let channel_id = channel_ids
        .first()
        .cloned()
        .expect("require_all_configured guarantees at least one id");

    // Validate required fields
    if req.session_id.trim().is_empty() || req.message_id.trim().is_empty() {
        return Err(ApiError::bad_request(
            "sessionId and messageId are required",
        ));
    }

    match req.feedback.as_deref() {
        Some("like") | Some("dislike") | None => {}
        Some(_) => {
            return Err(ApiError::bad_request(
                "feedback must be 'like', 'dislike', or null",
            ));
        }
    }

    state
        .sqlite
        .call(move |conn| {
            match req.feedback {
                None => {
                    // Idempotent: no error if the row doesn't exist.
                    conn.execute(
                        "DELETE FROM chat_feedback WHERE channel_id = ?1 AND session_id = ?2 AND message_id = ?3",
                        rusqlite::params![channel_id, req.session_id, req.message_id],
                    )?;
                }
                Some(ref feedback) => {
                    conn.execute(
                        "INSERT OR REPLACE INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        rusqlite::params![
                            channel_id,
                            req.session_id,
                            req.message_id,
                            feedback,
                            req.user_message,
                            req.assistant_message,
                        ],
                    )?;
                }
            }
            Ok::<(), rusqlite::Error>(())
        })
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(StatusCode::NO_CONTENT)
}

/// Query channel-scoped feedback list with pagination and optional filtering.
///
/// Requires Bearer Token authentication (registered in doc_router).
/// Only returns feedback records belonging to the requested `channelId`.
#[utoipa::path(
    get,
    path = "/api/chat/feedback",
    tag = "chat",
    security(("bearer_auth" = [])),
    params(FeedbackQueryParams),
    responses(
        (status = 200, description = "Feedback list", body = FeedbackListResponse),
        (status = 400, description = "Missing or invalid channelId", body = crate::application::http::errors::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::errors::ErrorResponse),
        (status = 500, description = "Database error", body = crate::application::http::errors::ErrorResponse)
    )
)]
pub async fn list_feedback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FeedbackQueryParams>,
) -> Result<Json<FeedbackListResponse>, ApiError> {
    let channel_id = state
        .channels_config
        .require_configured(&params.channel_id)
        .map_err(|e| match e {
            ChannelValidationError::Empty => ApiError::bad_request("channelId 不能为空"),
            ChannelValidationError::NotConfigured(id) => {
                ApiError::bad_request(format!("频道 {id} 未配置"))
            }
        })?
        .to_string();

    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);

    state
        .sqlite
        .call(move |conn| {
            let filter_feedback = params.feedback.as_deref();
            let base_where = "WHERE channel_id = ?1";

            let (count_sql, data_sql) = if filter_feedback.is_some() {
                (
                    format!(
                        "SELECT COUNT(*) FROM chat_feedback {base_where} AND feedback = ?2"
                    ),
                    format!(
                        "SELECT id, channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                         FROM chat_feedback {base_where} AND feedback = ?2 \
                         ORDER BY created_at DESC LIMIT ?3 OFFSET ?4"
                    ),
                )
            } else {
                (
                    format!("SELECT COUNT(*) FROM chat_feedback {base_where}"),
                    format!(
                        "SELECT id, channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                         FROM chat_feedback {base_where} \
                         ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
                    ),
                )
            };

            let total = if let Some(feedback) = filter_feedback {
                conn.query_row(
                    &count_sql,
                    rusqlite::params![channel_id, feedback],
                    |row| row.get::<_, i64>(0),
                )?
            } else {
                conn.query_row(&count_sql, rusqlite::params![channel_id], |row| {
                    row.get::<_, i64>(0)
                })?
            };

            let row_to_item = |row: &rusqlite::Row| -> Result<FeedbackItem, rusqlite::Error> {
                Ok(FeedbackItem {
                    id: row.get(0)?,
                    channel_id: row.get(1)?,
                    session_id: row.get(2)?,
                    message_id: row.get(3)?,
                    feedback: row.get(4)?,
                    user_message: row.get(5)?,
                    assistant_message: row.get(6)?,
                    created_at: row.get(7)?,
                })
            };

            let items = if let Some(feedback) = filter_feedback {
                conn.prepare(&data_sql)?.query_map(
                    rusqlite::params![channel_id, feedback, limit, offset],
                    row_to_item,
                )?
                .collect::<Result<Vec<_>, _>>()?
            } else {
                conn.prepare(&data_sql)?
                    .query_map(rusqlite::params![channel_id, limit, offset], row_to_item)?
                    .collect::<Result<Vec<_>, _>>()?
            };

            Ok::<FeedbackListResponse, rusqlite::Error>(FeedbackListResponse { items, total })
        })
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
