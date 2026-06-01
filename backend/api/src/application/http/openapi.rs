use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::OpenApi;

use crate::application::http::handlers::chat::ChatRequest;
use crate::application::http::handlers::document::{
    DocumentListItem, DocumentListResponse, PublishDocumentResponse, UploadDocumentResponse,
};

/// OpenAPI 文档定义
///
/// utoipa 会根据此 derive 宏自动生成 OpenAPI 3.0 规范。
/// 新增路由后，需要在此处的 paths() 中注册对应的处理函数，
/// 新增数据结构后，需要在 components(schemas()) 中注册。
///
/// 访问方式：
/// - enable_openapi = true 时，访问 /swagger 查看 Swagger UI
/// - enable_openapi = false 时，/swagger 返回 404
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Rwiki API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Rwiki API Documentation",
        license(name = "MIT")
    ),
    paths(
        crate::application::http::handlers::health_check,
        crate::application::http::handlers::document::upload_document,
        crate::application::http::handlers::document::list_documents,
        crate::application::http::handlers::document::delete_document,
        crate::application::http::handlers::document::publish_document,
        crate::application::http::handlers::document::unpublish_document,
        crate::application::http::handlers::chat::chat,
    ),
    components(
        schemas(
            crate::application::http::errors::ErrorResponse,
            UploadDocumentResponse,
            DocumentListItem,
            DocumentListResponse,
            PublishDocumentResponse,
            ChatRequest,
        )
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "documents", description = "Document upload, listing, deletion, and publishing"),
        (name = "chat", description = "Knowledge base chat with SSE streaming")
    ),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;

/// 安全方案附加组件
///
/// 在 OpenAPI 文档中注册 Bearer Token 认证方案。
/// 此方案使用静态 API Token（非 JWT），通过配置文件或环境变量设置。
/// 当有需要认证的接口时，在 utoipa::path 注解中添加
/// security(("bearer_auth" = [])) 即可。
struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("Token")
                        .description(Some("Static API token (not JWT)"))
                        .build(),
                ),
            )
        }
    }
}
