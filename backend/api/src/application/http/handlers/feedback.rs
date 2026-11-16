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
use rwiki_core::infrastructure::storage::{MetadataStore, NewFeedback};

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRequest {
    /// Channel identifiers; required (re-validated in the handler to return 400).
    /// Supports single or multiple channels.
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
    /// Channel the feedback belongs to
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
    /// Channel identifier; required
    pub channel_id: String,
    pub feedback: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

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
        .store
        .upsert_feedback(NewFeedback {
            channel_id,
            session_id: req.session_id,
            message_id: req.message_id,
            feedback: req.feedback,
            user_message: req.user_message,
            assistant_message: req.assistant_message,
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

    let page = state
        .store
        .list_feedback(
            &channel_id,
            params.feedback.as_deref(),
            limit as i64,
            offset as i64,
        )
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    let items = page
        .items
        .into_iter()
        .map(|record| FeedbackItem {
            id: record.id,
            channel_id: record.channel_id,
            session_id: record.session_id,
            message_id: record.message_id,
            feedback: record.feedback,
            user_message: record.user_message,
            assistant_message: record.assistant_message,
            created_at: record.created_at,
        })
        .collect();

    Ok(Json(FeedbackListResponse {
        items,
        total: page.total,
    }))
}
