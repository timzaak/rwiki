pub mod application;
pub mod config;

pub use application::http::AppState;
pub use application::http::{create_api_routes, ApiDoc};
pub use config::ApiConfig;

use anyhow::Result;
use utoipa::OpenApi;

/// 导出 OpenAPI 规范到文件
///
/// 通过命令行参数 `--export-openapi <path>` 触发，
/// 将 OpenAPI JSON 写入指定路径，供前端生成 TypeScript 客户端。
pub fn export_openapi(output_path: &str) -> Result<()> {
    let openapi = ApiDoc::openapi();
    let json = serde_json::to_string_pretty(&openapi)?;
    std::fs::write(output_path, json)?;
    println!("OpenAPI spec exported to {}", output_path);
    Ok(())
}
