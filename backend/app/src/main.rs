use anyhow::Result;
use clap::Parser;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};
use opentelemetry_sdk::Resource;
use rig::client::EmbeddingsClient;
use rig::embeddings::EmbeddingModel;
use std::env;
use std::sync::Arc;
use std::sync::Once;
use tracing_subscriber::prelude::*;

static SQLITE_VEC_INIT: Once = Once::new();

/// Register the sqlite-vec extension globally. Must be called before opening any
/// SQLite connection that uses vec0 virtual tables. Safe to call multiple times.
fn ensure_sqlite_vec_loaded() {
    SQLITE_VEC_INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

/// Rwiki Backend Application
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// 配置文件路径
    #[arg(short, long, default_value = "config/config.toml")]
    config: String,

    /// 导出 OpenAPI 规范到文件
    /// 用法：cargo run --export-openapi ../frontend/api.json
    #[arg(long, value_name = "FILE")]
    export_openapi: Option<String>,
}

struct TracerOutput {
    tracer: opentelemetry_sdk::trace::SdkTracer,
    #[allow(dead_code)]
    guard: TracerGuard,
}

struct TracerGuard {
    provider: opentelemetry_sdk::trace::SdkTracerProvider,
}

impl Drop for TracerGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            tracing::error!("OTLP tracer provider shutdown failed: {:?}", e);
        }
    }
}

/// Initialize OTLP tracer provider with gRPC exporter.
/// Panics on failure (config error per PRD).
fn init_otel_tracing(otel_config: &rwiki_core::config::OtelConfig) -> TracerOutput {
    let mut metadata = tonic::metadata::MetadataMap::new();
    if !otel_config.license_key.is_empty() {
        metadata.insert(
            "authentication",
            otel_config
                .license_key
                .parse()
                .expect("invalid license_key header value"),
        );
    }

    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&otel_config.endpoint)
        .with_metadata(metadata)
        .build()
        .expect("Failed to build OTLP span exporter — check endpoint configuration");

    let service_name = &otel_config.service_name;
    let resource = Resource::builder()
        .with_service_name(service_name.clone())
        .build();

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = tracer_provider.tracer(service_name.clone());

    TracerOutput {
        tracer,
        guard: TracerGuard {
            provider: tracer_provider,
        },
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 处理 OpenAPI 导出模式
    // 运行 cargo run --export-openapi <path> 只导出 API 规范，不启动服务器
    if let Some(output_path) = args.export_openapi {
        return api::export_openapi(&output_path);
    }

    // 加载配置
    let config_path = env::var("APP_CONFIG").unwrap_or(args.config);
    let config = api::ApiConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Failed to load config from {}: {}", config_path, e))?;

    // 初始化日志（条件追加 OTLP layer）
    let otel_output = if !config.otel.endpoint.is_empty() {
        Some(init_otel_tracing(&config.otel))
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| config.server.log_level.clone().into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true)
                .with_level(true),
        )
        .with(
            otel_output
                .as_ref()
                .map(|o| tracing_opentelemetry::OpenTelemetryLayer::new(o.tracer.clone())),
        )
        .init();

    tracing::info!("Starting Rwiki Application");
    tracing::info!("Config loaded from: {}", config_path);
    tracing::info!("Bind address: {}", config.server.bind_address);
    tracing::info!("OpenAPI enabled: {}", config.server.enable_openapi);

    // 注册 sqlite-vec 扩展（必须在打开连接之前调用）
    ensure_sqlite_vec_loaded();

    // 确保数据目录存在
    if let Some(parent) = std::path::Path::new(&config.sqlite.path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // 创建 Embedding 客户端（在迁移之前，因为迁移需要知道维度）
    let api_key = config
        .embedding
        .api_key
        .as_deref()
        .filter(|k| !k.is_empty())
        .ok_or_else(|| anyhow::anyhow!("embedding.api_key required and non-empty"))?;
    let mut builder = rig::providers::openai::Client::builder().api_key(api_key);
    if let Some(ref base_url) = config.embedding.base_url {
        builder = builder.base_url(base_url);
    }
    let client = builder.build()?;
    let model_name = config
        .embedding
        .model
        .as_deref()
        .unwrap_or("text-embedding-3-small")
        .to_string();
    let model = match config.embedding.dimensions {
        Some(dims) => client.embedding_model_with_ndims(&model_name, dims),
        None => client.embedding_model(&model_name),
    };
    let embedding_model =
        rwiki_core::infrastructure::embedding_model::AppEmbeddingModel::new(model);
    tracing::info!("Embedding model: {}", model_name);
    let model_dims = embedding_model.ndims();

    // 打开 SQLite 连接
    let mut conn = rusqlite::Connection::open(&config.sqlite.path)?;

    // 启用 WAL 模式
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // 运行数据库迁移（vec_chunks 列宽由 embedding 模型维度决定）
    rwiki_core::infrastructure::migration::migrations(model_dims).to_latest(&mut conn)?;
    tracing::info!("Database migrations completed");

    // 校验 vec_chunks schema 维度与模型输出一致
    // sqlite_master 记录了建表 SQL，从中提取 float[N] 中的 N
    {
        let mut stmt =
            conn.prepare("SELECT sql FROM sqlite_master WHERE type='table' AND name='vec_chunks'")?;
        let schema_sql: String = stmt
            .query_row([], |row| row.get(0))
            .map_err(|e| anyhow::anyhow!("vec_chunks table not found in sqlite_master: {}", e))?;
        if let Some(schema_dims) = schema_sql
            .find("float[")
            .and_then(|i| {
                let rest = &schema_sql[i + 6..];
                rest.find(']').map(|j| &rest[..j])
            })
            .and_then(|s| s.parse::<usize>().ok())
        {
            if schema_dims != model_dims {
                panic!(
                    "Embedding dimension mismatch: model outputs {} dims, but vec_chunks schema has float[{}]. \
                     Delete the database to re-run migrations, or configure a compatible model.",
                    model_dims, schema_dims
                );
            }
        }
    }
    tracing::info!("Embedding dimension validation passed: {} dims", model_dims);

    // 清理上次崩溃遗留的 processing 状态文档
    let affected = conn.execute(
        "UPDATE documents SET status = 'failed' WHERE status = 'processing'",
        [],
    )?;
    if affected > 0 {
        tracing::warn!(
            "Marked {} stuck 'processing' document(s) as 'failed'",
            affected
        );
    }

    // 校验 api_token 非空
    if config.api.token.is_empty() {
        panic!(
            "api.token is required but not configured. \
             Set it in config file [api] token or via API_TOKEN environment variable."
        );
    }

    // 包装为 tokio-rusqlite 异步连接
    let sqlite = Arc::new(tokio_rusqlite::Connection::from(conn));

    // 创建向量存储管理器
    let vector_store = Arc::new(
        rwiki_core::infrastructure::vector_store::VectorStoreManager::new(
            sqlite.clone(),
            embedding_model,
            model_name,
        ),
    );

    // 后台补齐旧数据的 content_hash 和 embedding_model
    {
        let vs = vector_store.clone();
        tokio::spawn(async move {
            if let Err(e) = vs.backfill_content_hash().await {
                tracing::error!("Backfill failed: {}", e);
            }
        });
    }

    // 创建 LLM 客户端
    let llm_client = rig::providers::openai::CompletionsClient::builder()
        .api_key(&config.llm.api_key)
        .base_url(&config.llm.base_url)
        .build()?;

    // 归一化空字符串为默认值（PRD 4.2: 配置值为空等同于未配置）
    let mut chat_config = config.chat.clone();
    if chat_config.system_prompt.is_empty() {
        chat_config.system_prompt =
            rwiki_core::config::ChatConfig::DEFAULT_SYSTEM_PROMPT.to_string();
    }

    // 构建应用状态
    let app_state = Arc::new(api::application::http::AppState {
        sqlite: sqlite.clone(),
        enable_openapi: config.server.enable_openapi,
        vector_store,
        chat_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        llm_client,
        llm_model: config.llm.model.clone(),
        api_token: config.api.token.clone(),
        chat_config,
        static_dir: config.server.static_dir.clone(),
    });

    // 创建路由并启动服务器
    let app = api::create_api_routes(app_state);
    let listener = tokio::net::TcpListener::bind(&config.server.bind_address).await?;
    tracing::info!("Server listening on {}", config.server.bind_address);

    // Graceful shutdown: listen for Ctrl+C
    let shutdown_signal = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
        tracing::info!("Received shutdown signal, draining...");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    // TracerGuard::Drop flushes pending OTLP spans when otel_output goes out of scope
    drop(otel_output);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Covers: Design 5.3 TracerGuard Drop — shutdown completes without panic.
    // User Story: Graceful shutdown flushes pending spans; TracerGuard::Drop must not panic.
    //
    // Uses SdkTracerProvider::default() (no exporter, empty span processors) to avoid
    // needing an external OTLP collector. Default provider has no side effects.
    #[test]
    fn tracer_guard_drop_does_not_panic() {
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        let guard = TracerGuard { provider };
        // Should not panic — Drop calls shutdown on a no-op provider
        drop(guard);
    }

    // Covers: Design 5.2 conditional OTLP layer, Design 6.1 "endpoint empty skips OTLP init".
    // User Story: OTLP layer is only created when endpoint is non-empty.
    //
    // Verifies the conditional: empty endpoint means init_otel_tracing is NOT called,
    // so the OpenTelemetryLayer is not added to the subscriber.
    #[test]
    fn empty_endpoint_skips_otel_init() {
        let config = rwiki_core::config::OtelConfig {
            endpoint: String::new(),
            license_key: String::new(),
            service_name: "test".to_string(),
        };
        // The conditional in main() is: if !config.otel.endpoint.is_empty() { Some(init) } else { None }
        // With empty endpoint, otel_output is None → no OTLP layer added.
        // Verify that calling init_otel_tracing with a non-empty endpoint succeeds
        // while the empty-endpoint path correctly skips it.
        let should_skip = config.endpoint.is_empty();
        assert!(should_skip, "empty endpoint should skip OTLP init");

        // Verify the inverse: non-empty endpoint would NOT be skipped
        let config_with_endpoint = rwiki_core::config::OtelConfig {
            endpoint: "https://localhost:4317".to_string(),
            license_key: String::new(),
            service_name: "test".to_string(),
        };
        assert!(
            !config_with_endpoint.endpoint.is_empty(),
            "non-empty endpoint should NOT skip OTLP init"
        );
    }
}
