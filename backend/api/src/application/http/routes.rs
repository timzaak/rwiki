use crate::application::http::{handlers, openapi::ApiDoc, state::AppState};
use axum::http::Method;
use axum::http::StatusCode;
use axum::routing::{delete, get, patch, post};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use utoipa::OpenApi;

/// 创建 API 路由
///
/// 这是路由注册的入口点。所有 API 路由都在此函数中注册。
///
/// 路由结构：
/// - /health — 健康检查（始终可用，无需鉴权）
/// - /api/documents/upload — 上传 xlsx 文档（需鉴权）
/// - /api/documents — 文档列表（需鉴权）
/// - /api/documents/{documentId} — 删除文档（需鉴权）
/// - /api/documents/{documentId}/publish — 发布文档（需鉴权）
/// - /api/documents/{documentId}/unpublish — 取消发布文档（需鉴权）
/// - /api/chat — SSE 流式聊天（无需鉴权）
/// - /swagger — Swagger UI（仅 enable_openapi = true 时可用）
/// - 其他路径 — 代理到前端静态文件
pub fn create_api_routes(state: std::sync::Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers(Any);

    // Document routes behind auth middleware
    let doc_router = Router::new()
        .route(
            "/api/documents/upload",
            post(handlers::document::upload_document),
        )
        .route("/api/documents", get(handlers::document::list_documents))
        .route(
            "/api/documents/{documentId}",
            delete(handlers::document::delete_document),
        )
        .route(
            "/api/documents/{documentId}/publish",
            patch(handlers::document::publish_document),
        )
        .route(
            "/api/documents/{documentId}/unpublish",
            patch(handlers::document::unpublish_document),
        )
        .route("/api/chat/feedback", get(handlers::feedback::list_feedback))
        .route("/api/eval/query", post(handlers::eval::eval_query))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::application::http::middleware::auth_middleware,
        ))
        .with_state(state.clone());

    // Unprotected routes: health check and chat
    let app = Router::new()
        .route("/health", get(handlers::health_check))
        .route("/api/chat", post(handlers::chat::chat))
        .route("/api/chat/suggestions", get(handlers::chat::suggestions))
        .route(
            "/api/chat/feedback",
            post(handlers::feedback::submit_feedback),
        )
        .merge(doc_router)
        .layer(cors)
        .with_state(state.clone());

    // OpenAPI 开关：根据配置决定是否暴露 Swagger 文档
    let app = if state.enable_openapi {
        let swagger_ui = utoipa_swagger_ui::SwaggerUi::new("/swagger")
            .url("/api-docs/openapi.json", ApiDoc::openapi());
        app.merge(swagger_ui)
    } else {
        // 关闭时返回 404，避免暴露 API 文档
        app.route("/swagger", get(|| async { StatusCode::NOT_FOUND }))
            .route("/swagger/", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/api-docs/openapi.json",
                get(|| async { StatusCode::NOT_FOUND }),
            )
    };

    // 托管静态文件（widget JS 等）
    if let Some(ref static_dir) = state.static_dir {
        tracing::info!("Serving static files from: {}", static_dir);
        app.nest_service("/widget", ServeDir::new(static_dir))
    } else {
        app
    }
}
