use std::sync::Arc;

use crate::application::http::errors::ApiError;
use crate::application::http::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{header, Request},
    middleware::Next,
    response::Response,
};

/// Bearer Token authentication middleware.
///
/// Extracts the Authorization header, validates `Bearer <token>` format,
/// and compares the token against `state.api_token`.
/// Returns 401 on missing or invalid token.
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, ApiError> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = auth_header
        .and_then(|v| {
            let (scheme, rest) = v.split_once(' ')?;
            if scheme.eq_ignore_ascii_case("Bearer") {
                Some(rest)
            } else {
                None
            }
        })
        .ok_or_else(|| ApiError::unauthorized("Unauthorized"))?;

    if token != state.api_token {
        return Err(ApiError::unauthorized("Unauthorized"));
    }

    Ok(next.run(req).await)
}
