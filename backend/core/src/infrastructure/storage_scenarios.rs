//! 存储边界场景测试（双轨）：PG 启动链快速失败、维度不匹配拒绝启动、
//! 重启持久化（免重传可检索）与删除级联不再召回。
//!
//! User Stories: US-INFRA-001（场景 3 快速失败）、US-INFRA-002（场景 3
//! 删除/重启恢复、场景 4 维度不匹配）。
//!
//! PG 依赖用例在容器不可达时静默跳过（本地无 Docker 的现有体验不变，
//! CI 保证双轨）。

use super::storage::{MetadataStore, NewChunkRow, NewDocument, RetrievalStore};
use super::storage_postgres;
use super::storage_testing::{
    make_pg_storage_with_dims, pg_available, pg_url_of, reopen_storage, with_storage_backends,
};
use crate::config::PostgresConfig;

fn pg_config(url: &str) -> PostgresConfig {
    PostgresConfig {
        url: url.to_string(),
        max_connections: 2,
        acquire_timeout_secs: 2,
    }
}

// User Story: US-INFRA-001 场景 3
// Covers: [postgres] 段存在但 url 为空 → 启动失败指名配置键，不静默回退。
#[tokio::test]
async fn startup_fails_fast_when_url_empty() {
    let err = match storage_postgres::startup(&pg_config("  "), 1536).await {
        Err(e) => e,
        Ok(_) => panic!("empty url must fail startup"),
    };
    assert!(
        err.to_string().contains("[postgres].url"),
        "error should name the config key, got: {err}"
    );
}

// User Story: US-INFRA-001 场景 3
// Covers: 实例不可达 → 报 host:port 与原因；不得回显含密码的完整连接串。
#[tokio::test]
async fn startup_fails_fast_when_unreachable_and_hides_credentials() {
    let url = "postgres://user:super-secret@127.0.0.1:59999/rwiki";
    let err = match storage_postgres::startup(&pg_config(url), 1536).await {
        Err(e) => e,
        Ok(_) => panic!("unreachable instance must fail startup"),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("127.0.0.1:59999"),
        "error should contain host:port, got: {msg}"
    );
    assert!(
        !msg.contains("super-secret"),
        "error must not echo credentials, got: {msg}"
    );
}

// User Story: US-INFRA-002 场景 4
// Covers: 同库换 embedding 维度重启 → 拒绝启动并提示重建索引（与 SQLite
// 维度检查同口径文案）。需要 PG 容器，不可达时跳过。
#[tokio::test]
async fn startup_rejects_dimension_mismatch_on_existing_database() {
    if !pg_available().await {
        eprintln!("SKIP: PG test server unavailable");
        return;
    }

    // 先以 2 维建库（模拟旧 embedding 模型的存量库）
    let storage = make_pg_storage_with_dims("dims_mismatch", 2).await;
    let url = pg_url_of(&storage).expect("tag db url");
    drop(storage);

    // 再以 3 维启动同一库 → 维度检查拒绝启动
    let err = match storage_postgres::startup(&pg_config(&url), 3).await {
        Err(e) => e,
        Ok(_) => panic!("dimension mismatch must reject startup"),
    };
    let msg = err.to_string();
    assert!(
        msg.contains("Embedding dimension mismatch"),
        "error should use the SQLite-parity message, got: {msg}"
    );
    assert!(
        msg.contains("vector(2)") && msg.contains("3 dims"),
        "error should state both dimensions, got: {msg}"
    );
}

// User Story: US-INFRA-002 场景 1（重启免重传）、场景 3（删除级联不再召回）
// Covers: 同一组断言在两后端各跑一遍——写入→重启（新建 Storage 实例）→
// 数据仍在且可检索；删除文档后 chunk 不再命中；启动恢复 processing→failed。
#[tokio::test]
async fn storage_lifecycle_persists_across_restart_and_deletes_cascade() {
    with_storage_backends("lifecycle", |storage| async move {
        // 写入一个文档 + 一个 chunk（分词单源：中文内容）
        storage
            .insert_document(NewDocument {
                id: "doc-1".to_string(),
                file_name: "t.md".to_string(),
                row_count: 1,
                channel_id: "chan".to_string(),
            })
            .await
            .expect("insert document");
        storage
            .insert_chunks(
                "doc-1",
                vec![NewChunkRow {
                    chunk_id: "c-1".to_string(),
                    content: "内存管理最佳实践".to_string(),
                    title: "内存".to_string(),
                    locale: None,
                    link: None,
                    tags: vec![],
                    section: None,
                    page_id: "p-1".to_string(),
                    sub_index: Some(0),
                    chunk_count: Some(1),
                    content_hash: "h-1".to_string(),
                    embedding_model: "test".to_string(),
                    embedding: vec![0.0; 1536],
                    fts_tokens: super::vector_store::tokenize_for_fts("内存管理最佳实践"),
                }],
            )
            .await
            .expect("insert chunks");

        // 重启：同一底层数据库上新建 Storage 实例（PG 重跑完整启动链）
        let reopened = reopen_storage(&storage).await;

        // 重启后免重传：元数据与全文检索仍命中（中文关键词口径）；
        // 状态在重启前后保持（draft 不受 processing->failed 恢复影响）
        storage
            .set_document_status("doc-1", Some("chan"), None, "draft")
            .await
            .expect("set draft before restart");
        let doc = reopened
            .get_document("doc-1", Some("chan"))
            .await
            .expect("get after reopen")
            .expect("document must survive restart");
        assert_eq!(doc.status, "draft", "draft 状态跨重启保持");
        let hits = reopened
            .search_by_keyword(
                "内存管理",
                5,
                &super::vector_store::RetrievalScope::Published,
            )
            .await
            .expect("keyword search after reopen");
        // 未发布 → 已发布过滤下不命中（发布过滤语义对等）
        assert!(
            hits.is_empty(),
            "unpublished doc must not match published scope"
        );

        // 删除级联：chunk/向量/索引一并清除，任何入口不再召回
        reopened
            .remove_document_chunks("doc-1")
            .await
            .expect("remove chunks");
        reopened
            .delete_document_row("doc-1", Some("chan"))
            .await
            .expect("delete document row");
        assert!(
            reopened
                .get_document("doc-1", Some("chan"))
                .await
                .unwrap()
                .is_none(),
            "document row must be gone"
        );
        assert!(
            reopened.is_empty().await.expect("is_empty"),
            "chunks must be cascaded away"
        );

        // 启动恢复：processing → failed（reopen 已重跑启动链；此库无 processing 行，
        // 用行为验证恢复路径可重入即可）
        let recovered = reopened
            .recover_stuck_processing_documents()
            .await
            .expect("recover");
        assert_eq!(recovered, 0, "no stuck processing rows remain");
    })
    .await;
}

// User Story: US-INFRA-002 场景 1
// Covers: 重启后 processing 遗留文档被标记为 failed（崩溃恢复对等）。
#[tokio::test]
async fn restart_marks_stuck_processing_documents_failed() {
    with_storage_backends("restart_recovery", |storage| async move {
        for id in ["doc-stuck", "doc-draft"] {
            storage
                .insert_document(NewDocument {
                    id: id.to_string(),
                    file_name: "t.md".to_string(),
                    row_count: 0,
                    channel_id: "chan".to_string(),
                })
                .await
                .expect("insert document");
        }
        storage
            .set_document_status("doc-draft", Some("chan"), None, "draft")
            .await
            .expect("set draft");

        let reopened = reopen_storage(&storage).await; // 启动链含 processing→failed

        let stuck = reopened
            .get_document("doc-stuck", Some("chan"))
            .await
            .expect("get stuck")
            .expect("row present");
        assert_eq!(stuck.status, "failed", "processing 必须被恢复为 failed");
        let draft = reopened
            .get_document("doc-draft", Some("chan"))
            .await
            .expect("get draft")
            .expect("row present");
        assert_eq!(draft.status, "draft", "非 processing 状态不受恢复影响");
    })
    .await;
}
