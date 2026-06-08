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

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRequest {
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
pub struct FeedbackQueryParams {
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
                    // Cancel feedback: DELETE (idempotent — no error if row doesn't exist)
                    conn.execute(
                        "DELETE FROM chat_feedback WHERE session_id = ?1 AND message_id = ?2",
                        rusqlite::params![req.session_id, req.message_id],
                    )?;
                }
                Some(ref feedback) => {
                    // Submit feedback: UPSERT
                    conn.execute(
                        "INSERT OR REPLACE INTO chat_feedback (session_id, message_id, feedback, user_message, assistant_message) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
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

/// Query feedback list with pagination and optional filtering.
///
/// Requires Bearer Token authentication (registered in doc_router).
#[utoipa::path(
    get,
    path = "/api/chat/feedback",
    tag = "chat",
    security(("bearer_auth" = [])),
    params(FeedbackQueryParams),
    responses(
        (status = 200, description = "Feedback list", body = FeedbackListResponse),
        (status = 401, description = "Unauthorized", body = crate::application::http::errors::ErrorResponse),
        (status = 500, description = "Database error", body = crate::application::http::errors::ErrorResponse)
    )
)]
pub async fn list_feedback(
    State(state): State<Arc<AppState>>,
    Query(params): Query<FeedbackQueryParams>,
) -> Result<Json<FeedbackListResponse>, ApiError> {
    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0);

    state
        .sqlite
        .call(move |conn| {
            let filter_feedback = params.feedback.as_deref();

            let (count_sql, data_sql) = if filter_feedback.is_some() {
                (
                    "SELECT COUNT(*) FROM chat_feedback WHERE feedback = ?1",
                    "SELECT id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                     FROM chat_feedback WHERE feedback = ?1 \
                     ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
                )
            } else {
                (
                    "SELECT COUNT(*) FROM chat_feedback",
                    "SELECT id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                     FROM chat_feedback \
                     ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
                )
            };

            let total = if filter_feedback.is_some() {
                conn.query_row(count_sql, rusqlite::params![filter_feedback], |row| row.get::<_, i64>(0))?
            } else {
                conn.query_row(count_sql, [], |row| row.get::<_, i64>(0))?
            };

            let row_to_item = |row: &rusqlite::Row| -> Result<FeedbackItem, rusqlite::Error> {
                Ok(FeedbackItem {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    message_id: row.get(2)?,
                    feedback: row.get(3)?,
                    user_message: row.get(4)?,
                    assistant_message: row.get(5)?,
                    created_at: row.get(6)?,
                })
            };

            let items = if filter_feedback.is_some() {
                conn.prepare(data_sql)?
                    .query_map(rusqlite::params![filter_feedback, limit, offset], row_to_item)?
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                conn.prepare(data_sql)?
                    .query_map(rusqlite::params![limit, offset], row_to_item)?
                    .collect::<Result<Vec<_>, _>>()?
            };

            Ok::<FeedbackListResponse, rusqlite::Error>(FeedbackListResponse { items, total })
        })
        .await
        .map(Json)
        .map_err(|e| ApiError::internal(e.to_string()))
}
