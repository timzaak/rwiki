pub mod chat;
pub mod document;
pub mod feedback;

#[cfg(test)]
mod chat_scenarios;

#[cfg(test)]
mod chat_suggestions_scenarios;

#[cfg(test)]
mod document_status_scenarios;

#[cfg(test)]
mod markdown_upload_scenarios;

#[cfg(test)]
mod openapi_upload_scenarios;

#[cfg(test)]
mod feedback_scenarios;

#[cfg(test)]
mod metrics_scenarios;

#[cfg(test)]
mod rerank_scenarios;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use rwiki_core::domain::health::HealthStatus;
use std::sync::Arc;

use crate::application::http::errors::ApiError;
use crate::application::http::state::AppState;

/// Health check endpoint.
///
/// GET /health
/// Returns API and database connectivity status.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "API is healthy", body = HealthStatus),
        (status = 503, description = "API is unhealthy", body = HealthStatus)
    )
)]
pub async fn health_check(
    State(state): State<Arc<AppState>>,
) -> Result<axum::response::Response, ApiError> {
    let db_status = match state
        .sqlite
        .call(|conn| {
            conn.query_row("SELECT 1", [], |_| Ok::<(), rusqlite::Error>(()))?;
            Ok::<(), rusqlite::Error>(())
        })
        .await
    {
        Ok(()) => "connected".to_string(),
        Err(e) => {
            tracing::error!("Database health check failed: {}", e);
            format!("error: {}", e)
        }
    };

    let status = HealthStatus::new().with_database(&db_status);

    if status.is_healthy() {
        Ok(Json(status).into_response())
    } else {
        Ok((StatusCode::SERVICE_UNAVAILABLE, Json(status)).into_response())
    }
}
