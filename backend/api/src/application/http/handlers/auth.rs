use axum::http::StatusCode;

/// Token verification endpoint.
///
/// This endpoint is protected by auth_middleware. If the request reaches this handler,
/// the token is valid and we return 204 No Content.
/// If the token is missing or invalid, the middleware returns 401 Unauthorized.
#[utoipa::path(
    get,
    path = "/api/auth/verify",
    tag = "auth",
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "Token valid"),
        (status = 401, description = "Unauthorized", body = crate::application::http::errors::ErrorResponse),
    )
)]
pub async fn verify_token() -> StatusCode {
    StatusCode::NO_CONTENT
}
