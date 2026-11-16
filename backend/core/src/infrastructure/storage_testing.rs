//! 存储测试工厂（`test-support` feature 门控，api/app 的 dev-dependencies 启用）。
//!
//! 双轨测试矩阵的唯一建库入口：触库场景测试经 `make_storage` 取得
//! `Storage`（SQLite 内存库 / PG 容器按环境探测）；DDL 守卫约束继续成立
//! （schema 只能经迁移/启动链建立，工厂内的 INSERT/SELECT 播种与校验
//! 不属于 DDL）。
//!
//! 双轨运行方式：`RWIKI_TEST_STORAGE=postgres` 时 `make_storage` 返回 PG
//! 轨道（同一组场景断言以第二次套件运行覆盖两后端）；不设或 PG 不可达时
//! 保持 SQLite（本地无容器的现有体验不变）。`with_storage_backends` 供
//! PG 专项用例在单测试内显式双轨执行。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sqlx::Row;

use super::storage::{FeedbackRecord, Storage};
use super::storage_sqlite::SqliteStore;

/// PG 测试服务器连接串（test-start.py 的 rwiki-test-postgres 容器）。
const DEFAULT_PG_TEST_URL: &str = "postgres://postgres:test@127.0.0.1:54329/rwiki_test";

fn pg_test_url() -> String {
    std::env::var("RWIKI_TEST_POSTGRES_URL").unwrap_or_else(|_| DEFAULT_PG_TEST_URL.to_string())
}

/// 请求 PG 轨道（套件级双轨开关：第二次运行时置 postgres）。
fn wants_pg_track() -> bool {
    std::env::var("RWIKI_TEST_STORAGE")
        .map(|v| v.eq_ignore_ascii_case("postgres"))
        .unwrap_or(false)
}

/// 每 make_storage 调用唯一编号（配合进程 id 保证并行测试的库级隔离）。
static PG_DB_SEQ: AtomicU64 = AtomicU64::new(0);

/// 建一个全新 `Storage` 实例。`tag` 用于 PG 轨道的按标签库级隔离
/// （`rwiki_test_<tag>_<pid>_<seq>`）；SQLite 轨道为独立内存库。
///
/// SQLite 分支：`open_in_memory` + WAL + `migrations(1536)` + sqlite-vec
/// 注册 + tokio_rusqlite 包装。
///
/// PG 分支：读取 `RWIKI_TEST_POSTGRES_URL`（缺省与 test-start.py 容器一致），
/// 每 tag 独立数据库（DROP + CREATE 后跑完整启动链：迁移 + vec DDL +
/// 维度检查），返回 `Storage::Postgres`。
pub async fn make_storage(tag: &str) -> Storage {
    if wants_pg_track() && pg_available().await {
        return make_pg_storage(tag).await;
    }
    make_sqlite_storage().await
}

/// 建一个已迁移的内存 SQLite 连接（open_in_memory + WAL + `migrations(dims)`）。
/// sqlite-vec 扩展须先经 `storage_sqlite::ensure_sqlite_vec_loaded()` 注册。
/// core 源内各测试模块与 make_sqlite_storage 共用的唯一定义点。
pub fn migrated_memory_conn(dims: usize) -> rusqlite::Connection {
    let mut conn = rusqlite::Connection::open_in_memory().expect("in-memory SQLite should open");
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL mode should be settable");
    super::migration::migrations(dims)
        .to_latest(&mut conn)
        .expect("migrations should run");
    conn
}

/// SQLite 轨道建库（始终可用）。
pub async fn make_sqlite_storage() -> Storage {
    super::storage_sqlite::ensure_sqlite_vec_loaded();

    Storage::Sqlite(SqliteStore::new(Arc::new(
        tokio_rusqlite::Connection::from(
            migrated_memory_conn(1536), // matches text-embedding-3-small default
        ),
    )))
}

/// PG 服务器是否可达（test-start.py 容器探活；不可达时静默退化为 SQLite）。
pub async fn pg_available() -> bool {
    use sqlx::Connection;
    <sqlx::PgConnection as Connection>::connect(&pg_test_url())
        .await
        .is_ok()
}

async fn make_pg_storage(tag: &str) -> Storage {
    make_pg_storage_with_dims(tag, 1536).await
}

/// 建一个 PG 轨道 Storage；`dims` 为向量维度（维度不匹配场景需要非缺省维度）。
pub async fn make_pg_storage_with_dims(tag: &str, dims: usize) -> Storage {
    use sqlx::Connection;

    let base_url = pg_test_url();
    let seq = PG_DB_SEQ.fetch_add(1, Ordering::Relaxed);
    let db_name = format!(
        "rwiki_test_{}_{}_{}",
        sanitize_tag(tag),
        std::process::id(),
        seq
    );

    // 库级隔离：DROP（FORCE 断开残留连接）→ CREATE，随后对该库跑完整启动链
    let mut conn = sqlx::PgConnection::connect(&base_url)
        .await
        .expect("PG test server should be reachable");
    let drop_sql = format!(r#"DROP DATABASE IF EXISTS "{db_name}" WITH (FORCE)"#);
    sqlx::query(sqlx::AssertSqlSafe(drop_sql))
        .execute(&mut conn)
        .await
        .expect("drop stale test database");
    let create_sql = format!(r#"CREATE DATABASE "{db_name}""#);
    sqlx::query(sqlx::AssertSqlSafe(create_sql))
        .execute(&mut conn)
        .await
        .expect("create test database");
    drop(conn);

    // 以目标库连接串构造完整 PG 启动链（探测 → 迁移 → vec DDL → 维度 → 恢复）
    let tag_url = rewrite_db_in_url(&base_url, &db_name);
    let config = crate::config::PostgresConfig {
        url: tag_url,
        max_connections: 4,
        acquire_timeout_secs: 10,
    };
    let pg = super::storage_postgres::startup(&config, dims)
        .await
        .expect("PG test startup chain should succeed on a fresh database");
    Storage::Postgres(pg)
}

/// 测试专用：取 PG 轨道 storage 的连接串（SQLite 轨道返回 None）。
/// 直接读 PgStore 在 startup 时携带的连接串。
pub fn pg_url_of(storage: &Storage) -> Option<String> {
    match storage {
        Storage::Sqlite(_) => None,
        Storage::Postgres(pg) => Some(pg.test_url().to_string()),
    }
}

/// 把连接串尾部的库名替换为 `db_name`（保留账号/主机/参数）。
fn rewrite_db_in_url(url: &str, db_name: &str) -> String {
    let (head, query) = match url.split_once('?') {
        Some((h, q)) => (h, Some(q)),
        None => (url, None),
    };
    let mut head = head.trim_end_matches('/').to_string();
    if let Some(pos) = head.rfind('/') {
        // 保留 scheme://user@host:port/ 之前的部分
        let scheme_end = head.find("://").map(|i| i + 3).unwrap_or(0);
        if pos >= scheme_end {
            head.truncate(pos);
        }
    }
    head.push('/');
    head.push_str(db_name);
    if let Some(query) = query {
        head.push('?');
        head.push_str(query);
    }
    head
}

fn sanitize_tag(tag: &str) -> String {
    let cleaned: String = tag
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cleaned.chars().take(30).collect()
}

/// 在两个后端上各执行一遍同一组断言（PG 不可达时仅 SQLite）。
/// 供 PG 行为对等的显式双轨用例使用；场景套件级双轨由
/// `RWIKI_TEST_STORAGE=postgres` 的第二次运行承担。
pub async fn with_storage_backends<F, Fut>(tag: &str, f: F)
where
    F: Fn(Storage) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    f(make_sqlite_storage().await).await;
    if pg_available().await {
        f(make_pg_storage(tag).await).await;
    }
}

/// SQLite 轨道测试取回底层连接做数据预置/校验；PG 轨道返回 None
/// （双轨断言的预置/校验须走存储 trait 方法或本模块的方言助手）。
pub fn sqlite_handle(storage: &Storage) -> Option<Arc<tokio_rusqlite::Connection>> {
    match storage {
        Storage::Sqlite(s) => Some(s.conn().clone()),
        Storage::Postgres(_) => None,
    }
}

/// 以同一底层数据库新建 Storage 实例（模拟服务重启：PG 轨道对既有库
/// 重跑完整启动链——迁移幂等 + 维度检查 + processing→failed 恢复；
/// SQLite 轨道复用同一连接并执行启动恢复步骤，行为与启动链对等）。
pub async fn reopen_storage(storage: &Storage) -> Storage {
    match storage {
        Storage::Sqlite(s) => {
            let reopened = Storage::Sqlite(SqliteStore::new(s.conn().clone()));
            use super::storage::MetadataStore;
            reopened
                .recover_stuck_processing_documents()
                .await
                .expect("startup recovery on reopen");
            reopened
        }
        Storage::Postgres(_) => {
            let url = pg_url_of(storage).expect("PG storage url");
            let config = crate::config::PostgresConfig {
                url,
                max_connections: 4,
                acquire_timeout_secs: 10,
            };
            Storage::Postgres(
                super::storage_postgres::startup(&config, 1536)
                    .await
                    .expect("reopen PG storage should succeed on an existing database"),
            )
        }
    }
}

/// 播种孤儿 chunk 行（仅 chunk_metadata，无向量、无 FTS 命中）：绕过
/// is_empty 守卫且保证检索零命中（低相关「完全未命中」/ 评测空检索场景）。
/// PG 轨道的 FK 要求 documents 行已存在（调用方先经 trait insert_document）。
pub async fn seed_chunk_metadata_only(
    storage: &Storage,
    doc_id: &str,
    chunk_id: &str,
    content: &str,
    title: &str,
) {
    match storage {
        Storage::Sqlite(s) => {
            let handle = s.conn().clone();
            let (d, c, ct, t) = (
                doc_id.to_string(),
                chunk_id.to_string(),
                content.to_string(),
                title.to_string(),
            );
            handle
                .call(move |conn| {
                    conn.execute(
                        "INSERT INTO chunk_metadata (document_id, chunk_id, content, title) \
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![d, c, ct, t],
                    )?;
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .expect("seed orphan chunk (sqlite)");
        }
        Storage::Postgres(pg) => {
            sqlx::query(
                "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, tokens_text) \
                 VALUES ($1, $2, $3, $4, '')",
            )
            .bind(doc_id)
            .bind(chunk_id)
            .bind(content)
            .bind(title)
            .execute(pg.test_pool())
            .await
            .expect("seed orphan chunk (pg)");
        }
    }
}

/// 播种一条反馈记录（可回填 created_at，供时间过滤场景使用）。
#[allow(clippy::too_many_arguments)]
pub async fn seed_feedback(
    storage: &Storage,
    channel_id: &str,
    session_id: &str,
    message_id: &str,
    feedback: &str,
    user_message: &str,
    assistant_message: &str,
    created_at: Option<&str>,
) {
    match storage {
        Storage::Sqlite(s) => {
            let handle = s.conn().clone();
            let (sid, sess, mid, fb, um, am) = (
                channel_id.to_string(),
                session_id.to_string(),
                message_id.to_string(),
                feedback.to_string(),
                user_message.to_string(),
                assistant_message.to_string(),
            );
            let ca = created_at.map(str::to_string);
            handle
                .call(move |conn| {
                    match ca {
                        Some(ts) => conn.execute(
                            "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                            rusqlite::params![sid, sess, mid, fb, um, am, ts],
                        )?,
                        None => conn.execute(
                            "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message) VALUES (?1,?2,?3,?4,?5,?6)",
                            rusqlite::params![sid, sess, mid, fb, um, am],
                        )?,
                    };
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .expect("seed feedback (sqlite)");
        }
        Storage::Postgres(pg) => {
            let pool = pg.test_pool();
            let sql = match created_at {
                Some(_) => {
                    "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7::timestamptz)"
                }
                None => {
                    "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message) \
                     VALUES ($1,$2,$3,$4,$5,$6)"
                }
            };
            let mut q = sqlx::query(sql)
                .bind(channel_id)
                .bind(session_id)
                .bind(message_id)
                .bind(feedback)
                .bind(user_message)
                .bind(assistant_message);
            if let Some(ts) = created_at {
                q = q.bind(ts);
            }
            q.execute(pool).await.expect("seed feedback (pg)");
        }
    }
}

/// 读取一个文档的全部 chunk 原始行（chunk_id / page_id / sub_index /
/// chunk_count / content），供上传解析类场景校验分块结构。
/// chunk 行元组（chunk_id / page_id / sub_index / chunk_count / content）。
pub type ChunkRow = (String, String, Option<i64>, Option<i64>, String);

pub async fn fetch_chunk_rows(storage: &Storage, document_id: &str) -> Vec<ChunkRow> {
    match storage {
        Storage::Sqlite(s) => {
            let handle = s.conn().clone();
            let doc = document_id.to_string();
            handle
                .call(move |conn| {
                    let rows: Vec<ChunkRow> = conn
                        .prepare(
                            "SELECT chunk_id, page_id, sub_index, chunk_count, content \
                             FROM chunk_metadata WHERE document_id = ?1 ORDER BY rowid",
                        )?
                        .query_map(rusqlite::params![doc], |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
                            ))
                        })?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok::<_, rusqlite::Error>(rows)
                })
                .await
                .expect("fetch chunk rows (sqlite)")
        }
        Storage::Postgres(pg) => {
            let rows = sqlx::query(
                "SELECT chunk_id, page_id, sub_index, chunk_count, content \
                 FROM chunk_metadata WHERE document_id = $1 ORDER BY id",
            )
            .bind(document_id)
            .fetch_all(pg.test_pool())
            .await
            .expect("fetch chunk rows (pg)");
            rows.iter()
                .map(|row| {
                    (
                        row.get::<String, _>("chunk_id"),
                        row.get::<String, _>("page_id"),
                        row.get::<Option<i64>, _>("sub_index"),
                        row.get::<Option<i64>, _>("chunk_count"),
                        row.get::<String, _>("content"),
                    )
                })
                .collect()
        }
    }
}

/// 读取全部低相关记录（id / channel_id / query / top_score，按 id 升序），
/// 供跨频道计数与「最近一条」断言在 Rust 侧过滤。
pub async fn fetch_low_recall_rows(
    storage: &Storage,
) -> Vec<(i64, Option<String>, String, Option<f64>)> {
    match storage {
        Storage::Sqlite(s) => {
            let handle = s.conn().clone();
            handle
                .call(|conn| {
                    let rows: Vec<(i64, Option<String>, String, Option<f64>)> = conn
                        .prepare(
                            "SELECT id, channel_id, query, top_score FROM low_recall_records ORDER BY id",
                        )?
                        .query_map([], |row| {
                            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                        })?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok::<_, rusqlite::Error>(rows)
                })
                .await
                .expect("fetch low_recall rows (sqlite)")
        }
        Storage::Postgres(pg) => {
            let rows = sqlx::query(
                "SELECT id, channel_id, query, top_score FROM low_recall_records ORDER BY id",
            )
            .fetch_all(pg.test_pool())
            .await
            .expect("fetch low_recall rows (pg)");
            rows.iter()
                .map(|row| {
                    (
                        row.get::<i64, _>("id"),
                        row.get::<Option<String>, _>("channel_id"),
                        row.get::<String, _>("query"),
                        row.get::<Option<f64>, _>("top_score"),
                    )
                })
                .collect()
        }
    }
}

/// 播种一条低相关记录（可回填 created_at，供时间过滤场景使用）。
#[allow(clippy::too_many_arguments)]
pub async fn seed_low_recall_record(
    storage: &Storage,
    channel_id: &str,
    session_id: Option<&str>,
    query: &str,
    top_score: Option<f64>,
    result_count: i64,
    sources_json: &str,
    created_at: Option<&str>,
) {
    match storage {
        Storage::Sqlite(s) => {
            let handle = s.conn().clone();
            let (sid, sess, q, ts) = (
                channel_id.to_string(),
                session_id.map(str::to_string),
                query.to_string(),
                created_at.map(str::to_string),
            );
            let (rc, sj) = (result_count, sources_json.to_string());
            handle
                .call(move |conn| {
                    match ts {
                        Some(ts) => conn.execute(
                            "INSERT INTO low_recall_records                              (channel_id, session_id, query, top_score, result_count, sources, created_at)                              VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                            rusqlite::params![sid, sess, q, top_score, rc, sj, ts],
                        )?,
                        None => conn.execute(
                            "INSERT INTO low_recall_records                              (channel_id, session_id, query, top_score, result_count, sources)                              VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![sid, sess, q, top_score, rc, sj],
                        )?,
                    };
                    Ok::<(), rusqlite::Error>(())
                })
                .await
                .expect("seed low_recall record (sqlite)");
        }
        Storage::Postgres(pg) => {
            let sql = match created_at {
                Some(_) => {
                    "INSERT INTO low_recall_records                      (channel_id, session_id, query, top_score, result_count, sources, created_at)                      VALUES ($1,$2,$3,$4,$5,$6,$7::timestamptz)"
                }
                None => {
                    "INSERT INTO low_recall_records                      (channel_id, session_id, query, top_score, result_count, sources)                      VALUES ($1,$2,$3,$4,$5,$6)"
                }
            };
            let mut q = sqlx::query(sql)
                .bind(channel_id)
                .bind(session_id)
                .bind(query)
                .bind(top_score)
                .bind(result_count)
                .bind(sources_json);
            if let Some(ts) = created_at {
                q = q.bind(ts);
            }
            q.execute(pg.test_pool())
                .await
                .expect("seed low_recall record (pg)");
        }
    }
}

/// 读取一个频道全部反馈行：走 MetadataStore::list_feedback（生产读路径），
/// 断言在 Rust 侧按 (session_id, message_id) 过滤。
pub async fn fetch_feedback_rows(storage: &Storage, channel_id: &str) -> Vec<FeedbackRecord> {
    use super::storage::MetadataStore;
    storage
        .list_feedback(channel_id, None, i64::MAX, 0)
        .await
        .expect("list feedback rows")
        .items
}
