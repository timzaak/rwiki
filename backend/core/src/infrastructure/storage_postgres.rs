//! PostgreSQL 存储实现（sqlx + pgvector）。
//!
//! 方言映射决策：时间统一 `to_char` 输出与 SQLite
//! `strftime('%Y-%m-%dT%H:%M:%fZ','now')` 同格式；tags 分隔符 `\x1f`（PG
//! text 不允许 NUL）；FTS 写入 `tokens_text`（生成列派生 fts）；KNN 用
//! `<=>` cosine + HNSW 迭代扫描；feedback upsert 以事务内 DELETE+INSERT
//! 精确复刻 `INSERT OR REPLACE`（含 id 重分配）。

use std::collections::HashMap;
use std::time::Duration;

use pgvector::Vector;
use sqlx::postgres::{PgArguments, PgConnection, PgPool, PgPoolOptions, PgRow};
use sqlx::{AssertSqlSafe, Connection, Row};
use uuid::Uuid;

use crate::config::PostgresConfig;
use crate::domain::errors::CoreError;

use super::storage::{
    cosine_distance_to_score, DocumentRecord, FeedbackPage, FeedbackRecord, LowRecallFilter,
    LowRecallPage, LowRecallRecordRow, MetadataStore, NewChunkRow, NewDocument, NewFeedback,
    NewLowRecallRecord, RetrievalStore,
};
use super::vector_store::{NeighborChunk, RetrievalScope, SearchResult};

/// 与 SQLite 存储格式一致的 ISO-8601 输出（毫秒 3 位 + Z 后缀）。
const TS_FMT: &str = "YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"";

/// PostgreSQL 存储实现：内持连接池（url 为 startup 使用的连接串，供测试读取）。
#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
    /// 仅 test-support 构建经 test_url() 读取；生产构建无读者（避免测试回查
    /// current_database / 字符串手术重推导连接串）。
    #[allow(dead_code)]
    url: String,
}

fn db_err(e: sqlx::Error) -> CoreError {
    CoreError::DatabaseError(e.to_string())
}

/// scope 的绑定参数（数组类型按 `ANY($n)` 单参数绑定）。
#[derive(Debug)]
enum ScopeParam {
    Scalar(String),
    Array(Vec<String>),
    None,
}

/// scope 的一次性解析（片段 / 绑定参数 / 下一占位号三点必须保持一致，
/// 单点求值避免多处 match 漂移）：
/// - 片段为纯谓词（不带前导 AND）：调用侧以 `WHERE {fragment}` 或
///   `... AND {fragment}` 拼接，绑定参数按 `$n` 编号；
/// - 数组参数按 `ANY($n)` 单参数绑定；
/// - `next` 为片段占用占位号之后的下一个编号（无参数时等于 `start`）。
fn scope_parts(scope: &RetrievalScope, start: usize) -> (String, ScopeParam, usize) {
    match scope {
        RetrievalScope::Channel(channel_id) => (
            format!("d.channel_id = ${start} AND d.status = 'published'"),
            ScopeParam::Scalar(channel_id.clone()),
            start + 1,
        ),
        RetrievalScope::Channels(ids) if !ids.is_empty() => (
            format!("d.channel_id = ANY(${start}) AND d.status = 'published'"),
            ScopeParam::Array(ids.clone()),
            start + 1,
        ),
        RetrievalScope::Collection(ids) if !ids.is_empty() => (
            format!("cm.document_id = ANY(${start})"),
            ScopeParam::Array(ids.clone()),
            start + 1,
        ),
        // Published / 空 Channels / 空 Collection 都回退到"仅已发布"
        RetrievalScope::Published | RetrievalScope::Channels(_) | RetrievalScope::Collection(_) => {
            (
                "d.status = 'published'".to_string(),
                ScopeParam::None,
                start,
            )
        }
    }
}

fn split_tags(tags_text: &str) -> Vec<String> {
    tags_text
        .split('\x1f')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn row_to_search_result(row: &PgRow, score: f64) -> SearchResult {
    SearchResult {
        chunk_id: row.get::<String, _>("chunk_id"),
        content: row.get::<String, _>("content"),
        title: row.get::<String, _>("title"),
        locale: row.get::<Option<String>, _>("locale"),
        link: row.get::<Option<String>, _>("link"),
        tags: split_tags(&row.get::<String, _>("tags")),
        section: row.get::<Option<String>, _>("section"),
        document_id: row.get::<String, _>("document_id"),
        page_id: row.get::<String, _>("page_id"),
        sub_index: row.get::<Option<i64>, _>("sub_index"),
        chunk_count: row.get::<Option<i64>, _>("chunk_count"),
        score,
    }
}

fn row_to_neighbor_chunk(row: &PgRow) -> NeighborChunk {
    NeighborChunk {
        chunk_id: row.get::<String, _>("chunk_id"),
        content: row.get::<String, _>("content"),
        sub_index: row.get::<Option<i64>, _>("sub_index"),
        chunk_count: row.get::<Option<i64>, _>("chunk_count"),
        title: row.get::<String, _>("title"),
        locale: row.get::<Option<String>, _>("locale"),
        link: row.get::<Option<String>, _>("link"),
        tags: split_tags(&row.get::<String, _>("tags")),
        section: row.get::<Option<String>, _>("section"),
        page_id: row.get::<String, _>("page_id"),
        document_id: row.get::<String, _>("document_id"),
    }
}

impl PgStore {
    fn new(pool: PgPool, url: String) -> Self {
        Self { pool, url }
    }

    /// 测试工厂的方言播种/校验助手取回连接池（不进入跨 crate 公开面）。
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_pool(&self) -> &PgPool {
        &self.pool
    }

    /// 测试专用：startup 时使用的连接串（不进入跨 crate 公开面）。
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_url(&self) -> &str {
        &self.url
    }

    /// 运行时 DDL：vec_chunks(vector(N)) + HNSW 索引（维度来自 embedding 配置，
    /// 无法进版本化迁移；幂等，每次启动执行，不记入 `_pg_migrations`）。
    async fn ensure_vec_objects(pool: &PgPool, ndims: usize) -> Result<(), sqlx::Error> {
        let create_table = format!(
            "CREATE TABLE IF NOT EXISTS vec_chunks (\
                chunk_row_id BIGINT PRIMARY KEY REFERENCES chunk_metadata(id) ON DELETE CASCADE,\
                embedding vector({ndims}) NOT NULL\
            )"
        );
        sqlx::query(AssertSqlSafe(create_table))
            .execute(pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_vec_chunks_embedding_hnsw \
             ON vec_chunks USING hnsw (embedding vector_cosine_ops) \
             WITH (m = 16, ef_construction = 64)",
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}

impl RetrievalStore for PgStore {
    async fn insert_chunks(
        &self,
        document_id: &str,
        rows: Vec<NewChunkRow>,
    ) -> Result<(), CoreError> {
        if rows.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await.map_err(db_err)?;
        for row in rows {
            let tags_text = row.tags.join("\x1f");
            let chunk_row_id: i64 = sqlx::query(
                "INSERT INTO chunk_metadata \
                    (document_id, chunk_id, content, title, locale, link, tags, section, \
                     page_id, sub_index, chunk_count, content_hash, embedding_model, tokens_text) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) RETURNING id",
            )
            .bind(document_id)
            .bind(&row.chunk_id)
            .bind(&row.content)
            .bind(&row.title)
            .bind(&row.locale)
            .bind(&row.link)
            .bind(tags_text)
            .bind(&row.section)
            .bind(&row.page_id)
            .bind(row.sub_index)
            .bind(row.chunk_count)
            .bind(&row.content_hash)
            .bind(&row.embedding_model)
            .bind(&row.fts_tokens)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?
            .get::<i64, _>("id");

            sqlx::query("INSERT INTO vec_chunks (chunk_row_id, embedding) VALUES ($1, $2)")
                .bind(chunk_row_id)
                .bind(Vector::from(row.embedding))
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    async fn find_cached_embeddings(
        &self,
        content_hashes: &[String],
        model_name: &str,
    ) -> Result<HashMap<String, Vec<f32>>, CoreError> {
        if content_hashes.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = sqlx::query(
            "SELECT cm.content_hash, v.embedding \
             FROM chunk_metadata cm \
             JOIN vec_chunks v ON v.chunk_row_id = cm.id \
             WHERE cm.content_hash = ANY($1) AND cm.embedding_model = $2",
        )
        .bind(content_hashes.to_vec())
        .bind(model_name)
        .fetch_all(&self.pool)
        .await
        .map_err(db_err)?;

        let mut result = HashMap::new();
        for row in rows {
            let hash: String = row.get("content_hash");
            // 同 hash 多行时任取一行（向量相同），与 SQLite GROUP BY 语义一致
            result
                .entry(hash)
                .or_insert_with(|| row.get::<Vector, _>("embedding").to_vec());
        }
        Ok(result)
    }

    /// PG 轨道为全新建库，content_hash 写入时总是携带，
    /// 无历史数据需要补齐——恒为 0 行，保持启动链单一路径。
    async fn backfill_content_hash(&self, _model_name: &str) -> Result<u64, CoreError> {
        Ok(0)
    }

    /// PG 侧 fts 为生成列，随 chunk 行写入自动派生，无需补齐——恒为 Ok(())，
    /// 与 backfill_content_hash 同走 trait 保持启动链单一路径。
    async fn backfill_fts_index(&self) -> Result<(), CoreError> {
        Ok(())
    }

    async fn search_by_vector(
        &self,
        query_vec: &[f64],
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let query_vec: Vec<f32> = query_vec.iter().map(|&v| v as f32).collect();
        // 取 cosine 距离，经 cosine_distance_to_score 归一到 SQLite vec0 L2
        // 分数标度（归一公式的唯一实现在 storage.rs 并由源内单测锚定）
        let (fragment, param, next) = scope_parts(scope, 2);
        let sql = format!(
            "SELECT cm.chunk_id, cm.content, cm.title, cm.locale, cm.link, cm.tags, cm.section, \
                    cm.document_id, cm.page_id, cm.sub_index, cm.chunk_count, \
                    (v.embedding <=> $1)::real AS distance \
             FROM vec_chunks v \
             JOIN chunk_metadata cm ON v.chunk_row_id = cm.id \
             JOIN documents d ON cm.document_id = d.id \
             WHERE {} \
             ORDER BY v.embedding <=> $1 \
             LIMIT ${}",
            fragment, next,
        );

        let mut q = sqlx::query(AssertSqlSafe(sql)).bind(Vector::from(query_vec));
        q = bind_scope_param(q, param);
        let rows = q
            .bind(top_k as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| CoreError::ProcessingError(format!("搜索失败: {e}")))?;

        Ok(rows
            .iter()
            .map(|row| {
                let distance: f32 = row.get("distance");
                row_to_search_result(row, cosine_distance_to_score(distance))
            })
            .collect())
    }

    async fn search_by_keyword(
        &self,
        fts_query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        // 入参为应用层预分词 token 串；plainto_tsquery 对其 AND 组合，
        // 与 FTS5 MATCH 空格隐含 AND 对等。ts_rank_cd 为 0..1 排序分
        // （语义与 bm25 一致：越高越相关；数值与 bm25 不同标度，不可直接比较）。
        let (fragment, param, next) = scope_parts(scope, 2);
        let sql = format!(
            "SELECT cm.chunk_id, cm.content, cm.title, cm.locale, cm.link, cm.tags, cm.section, \
                    cm.document_id, cm.page_id, cm.sub_index, cm.chunk_count, \
                    ts_rank_cd(cm.fts, q.tsq)::double precision AS score \
             FROM chunk_metadata cm \
             JOIN documents d ON cm.document_id = d.id \
             CROSS JOIN plainto_tsquery('simple'::regconfig, $1) AS q(tsq) \
             WHERE cm.fts @@ q.tsq AND {} \
             ORDER BY score DESC \
             LIMIT ${}",
            fragment, next,
        );

        let mut q = sqlx::query(AssertSqlSafe(sql)).bind(fts_query);
        q = bind_scope_param(q, param);
        let result = q.bind(top_k as i64).fetch_all(&self.pool).await;

        match result {
            Ok(rows) => Ok(rows
                .iter()
                .map(|row| row_to_search_result(row, row.get::<f64, _>("score")))
                .collect()),
            Err(e) => {
                // 索引缺失/查询失败按现状降级为空结果（对齐 SQLite 行为）
                tracing::warn!("FTS keyword search failed, degrading to empty results: {e}");
                Ok(Vec::new())
            }
        }
    }

    async fn get_neighbor_chunks(
        &self,
        page_id: &str,
        start_sub: i64,
        end_sub: i64,
        scope: &RetrievalScope,
    ) -> Result<Vec<NeighborChunk>, CoreError> {
        let (fragment, param, _next) = scope_parts(scope, 4);
        let sql = format!(
            "SELECT chunk_id, content, sub_index, chunk_count, title, locale, link, tags, \
                    section, page_id, document_id \
             FROM chunk_metadata cm \
             JOIN documents d ON cm.document_id = d.id \
             WHERE cm.page_id = $1 AND cm.sub_index >= $2 AND cm.sub_index < $3 \
               AND cm.chunk_count IS NOT NULL \
             AND {} \
             ORDER BY cm.sub_index",
            fragment,
        );

        let mut q = sqlx::query(AssertSqlSafe(sql))
            .bind(page_id)
            .bind(start_sub)
            .bind(end_sub);
        q = bind_scope_param(q, param);
        let rows = q.fetch_all(&self.pool).await.map_err(db_err)?;

        Ok(rows.iter().map(row_to_neighbor_chunk).collect())
    }

    async fn is_empty(&self) -> Result<bool, CoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_metadata")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("空库检查失败: {e}")))?;
        Ok(count == 0)
    }

    /// chunk_metadata 行删除经 FK 级联清除向量（vec_chunks ON DELETE CASCADE）；
    /// fts 为生成列随行派生，无独立清理对象。幂等：无行受影响为合法。
    async fn remove_document_chunks(&self, document_id: &str) -> Result<(), CoreError> {
        sqlx::query("DELETE FROM chunk_metadata WHERE document_id = $1")
            .bind(document_id)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("删除文档向量失败: {e}")))?;
        Ok(())
    }
}

impl MetadataStore for PgStore {
    async fn ping(&self) -> Result<(), CoreError> {
        let _: i32 = sqlx::query_scalar("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("健康检查失败: {e}")))?;
        Ok(())
    }

    async fn insert_document(&self, doc: NewDocument) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO documents (id, file_name, status, row_count, channel_id) \
             VALUES ($1, $2, 'processing', $3, $4)",
        )
        .bind(&doc.id)
        .bind(&doc.file_name)
        .bind(doc.row_count)
        .bind(&doc.channel_id)
        .execute(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("数据库写入失败: {e}")))?;
        Ok(())
    }

    async fn get_document(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<Option<DocumentRecord>, CoreError> {
        // 编号参数：$1 = id；有频道时 $2 = channel、$3 = 时间格式，否则 $2 = 时间格式
        let (sql, chan) = match channel_id {
            Some(_) => (
                "SELECT id, file_name, status, row_count, channel_id, error_message, \
                 to_char(created_at, $3) AS created_at \
                 FROM documents WHERE id = $1 AND channel_id = $2"
                    .to_string(),
                channel_id.map(str::to_string),
            ),
            None => (
                "SELECT id, file_name, status, row_count, channel_id, error_message, \
                 to_char(created_at, $2) AS created_at \
                 FROM documents WHERE id = $1"
                    .to_string(),
                None,
            ),
        };

        let mut q = sqlx::query(AssertSqlSafe(sql)).bind(document_id);
        if let Some(ch) = chan {
            q = q.bind(ch);
        }
        q = q.bind(TS_FMT);
        let row = q.fetch_optional(&self.pool).await.map_err(db_err)?;
        Ok(row.map(|row| DocumentRecord {
            id: row.get("id"),
            file_name: row.get("file_name"),
            status: row.get("status"),
            row_count: row.get::<i32, _>("row_count"),
            channel_id: row.get("channel_id"),
            error_message: row.get("error_message"),
            created_at: row.get("created_at"),
        }))
    }

    async fn list_documents_by_channel(
        &self,
        channel_id: &str,
    ) -> Result<Vec<DocumentRecord>, CoreError> {
        let rows = sqlx::query(
            "SELECT id, file_name, status, row_count, channel_id, error_message, \
                    to_char(created_at, $2) AS created_at \
             FROM documents WHERE channel_id = $1 \
             ORDER BY created_at DESC",
        )
        .bind(channel_id)
        .bind(TS_FMT)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| CoreError::DatabaseError(format!("获取文档列表失败: {e}")))?;

        Ok(rows
            .iter()
            .map(|row| DocumentRecord {
                id: row.get("id"),
                file_name: row.get("file_name"),
                status: row.get("status"),
                row_count: row.get::<i32, _>("row_count"),
                channel_id: row.get("channel_id"),
                error_message: row.get("error_message"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    async fn set_document_status(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
        from_status: Option<&str>,
        to_status: &str,
    ) -> Result<u64, CoreError> {
        let mut sql = String::from("UPDATE documents SET status = $1 WHERE id = $2");
        let mut next = 3usize;
        if channel_id.is_some() {
            sql.push_str(&format!(" AND channel_id = ${next}"));
            next += 1;
        }
        if from_status.is_some() {
            sql.push_str(&format!(" AND status = ${next}"));
        }

        let mut q = sqlx::query(AssertSqlSafe(sql))
            .bind(to_status)
            .bind(document_id);
        if let Some(ch) = channel_id {
            q = q.bind(ch);
        }
        if let Some(from) = from_status {
            q = q.bind(from);
        }
        let result = q.execute(&self.pool).await.map_err(db_err)?;
        Ok(result.rows_affected())
    }

    async fn mark_document_failed(
        &self,
        document_id: &str,
        error_message: &str,
    ) -> Result<(), CoreError> {
        sqlx::query("UPDATE documents SET status = 'failed', error_message = $1 WHERE id = $2")
            .bind(error_message)
            .bind(document_id)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("更新文档状态失败: {e}")))?;
        Ok(())
    }

    async fn document_statuses(
        &self,
        channel_id: &str,
        ids: &[Uuid],
    ) -> Result<Vec<(String, String)>, CoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let id_strs: Vec<String> = ids.iter().map(|u| u.to_string()).collect();
        let rows =
            sqlx::query("SELECT id, status FROM documents WHERE channel_id = $1 AND id = ANY($2)")
                .bind(channel_id)
                .bind(id_strs)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("批量状态更新失败: {e}")))?;

        Ok(rows
            .iter()
            .map(|row| (row.get::<String, _>("id"), row.get::<String, _>("status")))
            .collect())
    }

    async fn set_documents_status(
        &self,
        channel_id: &str,
        publish_ids: &[Uuid],
        unpublish_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, bool)>, CoreError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;

        let all_ids: Vec<String> = publish_ids
            .iter()
            .chain(unpublish_ids.iter())
            .map(|u| u.to_string())
            .collect();
        let rows =
            sqlx::query("SELECT id, status FROM documents WHERE channel_id = $1 AND id = ANY($2)")
                .bind(channel_id)
                .bind(all_ids)
                .fetch_all(&mut *tx)
                .await
                .map_err(db_err)?;

        let mut current_statuses = HashMap::new();
        for row in rows {
            let id: String = row.get("id");
            if let Ok(uuid) = Uuid::parse_str(&id) {
                current_statuses.insert(uuid, row.get::<String, _>("status"));
            }
        }

        // 守卫分类为两后端共享的纯逻辑（见 storage.rs）；绑定参数沿用 Vec<String> 口径
        let (applied_flags, valid_publish_ids, valid_unpublish_ids) =
            super::storage::classify_batch_status_guards(
                publish_ids,
                unpublish_ids,
                &current_statuses,
            );
        let valid_publish: Vec<String> = valid_publish_ids.iter().map(|u| u.to_string()).collect();
        let valid_unpublish: Vec<String> =
            valid_unpublish_ids.iter().map(|u| u.to_string()).collect();

        // 守卫更新：publish 要求 draft，unpublish 要求 published
        sqlx::query(
            "UPDATE documents SET status='published' \
             WHERE channel_id = $1 AND id = ANY($2) AND status='draft'",
        )
        .bind(channel_id)
        .bind(&valid_publish)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        sqlx::query(
            "UPDATE documents SET status='draft' \
             WHERE channel_id = $1 AND id = ANY($2) AND status='published'",
        )
        .bind(channel_id)
        .bind(&valid_unpublish)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        tx.commit().await.map_err(db_err)?;
        Ok(applied_flags)
    }

    async fn delete_document_row(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<u64, CoreError> {
        let result = match channel_id {
            Some(ch) => sqlx::query("DELETE FROM documents WHERE id = $1 AND channel_id = $2")
                .bind(document_id)
                .bind(ch)
                .execute(&self.pool)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("删除文档失败: {e}")))?,
            None => sqlx::query("DELETE FROM documents WHERE id = $1")
                .bind(document_id)
                .execute(&self.pool)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("删除文档失败: {e}")))?,
        };
        Ok(result.rows_affected())
    }

    async fn has_published_documents_for_channel(&self, channel_id: &str) -> bool {
        // SELECT 1 的 PG 类型是 int4，必须按 i32 解码；i64 解码失败会被
        // 下游 .ok() 吞掉并误报"无已发布文档"。
        let result: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM documents WHERE channel_id = $1 AND status = 'published' LIMIT 1",
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        result.is_some()
    }

    async fn has_published_documents_for_channels(&self, channel_ids: &[String]) -> bool {
        if channel_ids.is_empty() {
            return false;
        }
        let result: Option<i32> = sqlx::query_scalar(
            "SELECT 1 FROM documents \
             WHERE channel_id = ANY($1) AND status = 'published' LIMIT 1",
        )
        .bind(channel_ids.to_vec())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();
        result.is_some()
    }

    async fn count_documents_with_status(&self, status: &str) -> Result<u64, CoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM documents WHERE status = $1")
            .bind(status)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("指标统计失败: {e}")))?;
        Ok(count as u64)
    }

    async fn count_chunks(&self) -> Result<u64, CoreError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chunk_metadata")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("指标统计失败: {e}")))?;
        Ok(count as u64)
    }

    async fn recover_stuck_processing_documents(&self) -> Result<u64, CoreError> {
        let result =
            sqlx::query("UPDATE documents SET status = 'failed' WHERE status = 'processing'")
                .execute(&self.pool)
                .await
                .map_err(|e| CoreError::DatabaseError(format!("启动恢复失败: {e}")))?;
        Ok(result.rows_affected())
    }

    /// PG 三表（documents/chat_feedback/low_recall_records）channel_id 均
    /// NOT NULL，channel_id 为空的行不可能存在——恒为 Ok(())。
    async fn validate_historical_rows_have_channel_id(&self) -> Result<(), CoreError> {
        Ok(())
    }

    async fn upsert_feedback(&self, feedback: NewFeedback) -> Result<(), CoreError> {
        let mut tx = self.pool.begin().await.map_err(db_err)?;
        // 事务内 DELETE + INSERT 精确复刻 INSERT OR REPLACE（含 id 重分配）；
        // 不用 ON CONFLICT DO UPDATE（保持旧 id，构成可观察 API 差异）。
        sqlx::query(
            "DELETE FROM chat_feedback \
             WHERE (channel_id, session_id, message_id) = ($1, $2, $3)",
        )
        .bind(&feedback.channel_id)
        .bind(&feedback.session_id)
        .bind(&feedback.message_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

        if let Some(value) = feedback.feedback {
            sqlx::query(
                "INSERT INTO chat_feedback \
                    (channel_id, session_id, message_id, feedback, user_message, assistant_message) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(&feedback.channel_id)
            .bind(&feedback.session_id)
            .bind(&feedback.message_id)
            .bind(value)
            .bind(&feedback.user_message)
            .bind(&feedback.assistant_message)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        tx.commit().await.map_err(db_err)?;
        Ok(())
    }

    async fn list_feedback(
        &self,
        channel_id: &str,
        feedback: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<FeedbackPage, CoreError> {
        let (count_sql, data_sql) = match feedback {
            Some(_) => (
                "SELECT COUNT(*) FROM chat_feedback WHERE channel_id = $1 AND feedback = $2",
                "SELECT id, channel_id, session_id, message_id, feedback, user_message, \
                        assistant_message, to_char(created_at, $3) AS created_at \
                 FROM chat_feedback WHERE channel_id = $1 AND feedback = $2 \
                 ORDER BY created_at DESC LIMIT $4 OFFSET $5",
            ),
            None => (
                "SELECT COUNT(*) FROM chat_feedback WHERE channel_id = $1",
                "SELECT id, channel_id, session_id, message_id, feedback, user_message, \
                        assistant_message, to_char(created_at, $2) AS created_at \
                 FROM chat_feedback WHERE channel_id = $1 \
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            ),
        };

        let total: i64 = match feedback {
            Some(f) => sqlx::query_scalar(count_sql)
                .bind(channel_id)
                .bind(f)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?,
            None => sqlx::query_scalar(count_sql)
                .bind(channel_id)
                .fetch_one(&self.pool)
                .await
                .map_err(db_err)?,
        };

        let rows = match feedback {
            Some(f) => sqlx::query(data_sql)
                .bind(channel_id)
                .bind(f)
                .bind(TS_FMT)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?,
            None => sqlx::query(data_sql)
                .bind(channel_id)
                .bind(TS_FMT)
                .bind(limit)
                .bind(offset)
                .fetch_all(&self.pool)
                .await
                .map_err(db_err)?,
        };

        let items = rows
            .iter()
            .map(|row| FeedbackRecord {
                id: row.get::<i64, _>("id"),
                channel_id: row.get("channel_id"),
                session_id: row.get("session_id"),
                message_id: row.get("message_id"),
                feedback: row.get("feedback"),
                user_message: row.get("user_message"),
                assistant_message: row.get("assistant_message"),
                created_at: row.get("created_at"),
            })
            .collect();
        Ok(FeedbackPage { items, total })
    }

    async fn insert_low_recall_record(&self, record: NewLowRecallRecord) -> Result<(), CoreError> {
        sqlx::query(
            "INSERT INTO low_recall_records \
                (session_id, channel_id, query, top_score, result_count, sources) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&record.session_id)
        .bind(&record.channel_id)
        .bind(&record.query)
        .bind(record.top_score)
        .bind(record.result_count)
        .bind(&record.sources_json)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn list_low_recall_records(
        &self,
        filter: LowRecallFilter,
        limit: i64,
        offset: i64,
    ) -> Result<LowRecallPage, CoreError> {
        // 动态拼 WHERE；channel_id 首条过滤，from/to 以 ISO 字符串直转
        // timestamptz 比较（PG 原生解析 ISO 8601，与 SQLite 字典序比较同序）。
        let mut where_clauses: Vec<String> = vec!["channel_id = $1".to_string()];
        let mut idx = 2usize;

        if filter.min_score.is_some() {
            where_clauses.push(format!("top_score >= ${idx}"));
            idx += 1;
        }
        if filter.max_score.is_some() {
            where_clauses.push(format!("top_score <= ${idx}"));
            idx += 1;
        }
        if filter.from_ts.is_some() {
            where_clauses.push(format!("created_at >= ${idx}::timestamptz"));
            idx += 1;
        }
        if filter.to_ts.is_some() {
            where_clauses.push(format!("created_at <= ${idx}::timestamptz"));
            idx += 1;
        }

        let where_sql = where_clauses.join(" AND ");
        let count_sql = format!("SELECT COUNT(*) FROM low_recall_records WHERE {where_sql}");
        let data_sql = format!(
            "SELECT id, channel_id, session_id, query, top_score, result_count, sources, \
                    to_char(created_at, ${ts_fmt}) AS created_at \
             FROM low_recall_records WHERE {where_sql} \
             ORDER BY created_at DESC LIMIT ${limit} OFFSET ${offset}",
            ts_fmt = idx,
            limit = idx + 1,
            offset = idx + 2,
        );

        let mut count_q =
            sqlx::query_scalar::<_, i64>(AssertSqlSafe(count_sql)).bind(filter.channel_id.clone());
        if let Some(ref min) = filter.min_score {
            count_q = count_q.bind(*min);
        }
        if let Some(ref max) = filter.max_score {
            count_q = count_q.bind(*max);
        }
        if let Some(ref f) = filter.from_ts {
            count_q = count_q.bind(f.clone());
        }
        if let Some(ref t) = filter.to_ts {
            count_q = count_q.bind(t.clone());
        }
        let total: i64 = count_q.fetch_one(&self.pool).await.map_err(db_err)?;

        let mut data_q = sqlx::query(AssertSqlSafe(data_sql)).bind(filter.channel_id);
        if let Some(min) = filter.min_score {
            data_q = data_q.bind(min);
        }
        if let Some(max) = filter.max_score {
            data_q = data_q.bind(max);
        }
        if let Some(f) = filter.from_ts {
            data_q = data_q.bind(f);
        }
        if let Some(t) = filter.to_ts {
            data_q = data_q.bind(t);
        }
        let rows = data_q
            .bind(TS_FMT)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await
            .map_err(db_err)?;

        let items = rows
            .iter()
            .map(|row| LowRecallRecordRow {
                id: row.get::<i64, _>("id"),
                channel_id: row.get("channel_id"),
                session_id: row.get("session_id"),
                query: row.get("query"),
                top_score: row.get("top_score"),
                result_count: row.get::<i32, _>("result_count") as i64,
                sources_json: row.get("sources"),
                created_at: row.get("created_at"),
            })
            .collect();
        Ok(LowRecallPage { items, total })
    }
}

fn bind_scope_param<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, PgArguments>,
    param: ScopeParam,
) -> sqlx::query::Query<'q, sqlx::Postgres, PgArguments> {
    match param {
        ScopeParam::Scalar(s) => q.bind(s),
        ScopeParam::Array(v) => q.bind(v),
        ScopeParam::None => q,
    }
}

/// 提取连接串中的 host:port（错误信息不回显含密码的完整 url）。
fn host_port_of(url: &str) -> String {
    url.split("://")
        .nth(1)
        .and_then(|rest| rest.split('@').next_back())
        .and_then(|rest| rest.split('/').next().map(str::to_string))
        .unwrap_or_else(|| "<unknown>".to_string())
}

/// PG 启动链（快速失败、禁止静默回退）。
///
/// 顺序说明：先以单连接完成可达性与 pgvector 探测（保证报错文案明确指向
/// 部署前提而非 after_connect 的 GUC 失败），确认后建池（after_connect 统一
/// 注入会话参数），再走迁移 → vec DDL → 维度检查 → 崩溃恢复。
pub async fn startup(config: &PostgresConfig, model_dims: usize) -> anyhow::Result<PgStore> {
    if config.url.trim().is_empty() {
        anyhow::bail!("[postgres].url 未配置：启用 [postgres] 段时 url 必填且不能为空");
    }
    if config.max_connections == 0 {
        anyhow::bail!("[postgres].max_connections 必须大于等于 1");
    }
    if config.acquire_timeout_secs == 0 {
        anyhow::bail!("[postgres].acquire_timeout_secs 必须大于等于 1");
    }

    // 可达性探测失败只报 host:port 与原因，不回显完整 url
    let mut probe = <PgConnection as Connection>::connect(&config.url)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "PostgreSQL 不可达: {} ({e})，请检查 [postgres].url 与实例状态",
                host_port_of(&config.url)
            )
        })?;

    // pgvector 扩展探测：部署前提 ≥0.8（迭代扫描需要）
    async fn probe_version(conn: &mut PgConnection) -> anyhow::Result<Option<String>> {
        let v: Option<String> =
            sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'vector'")
                .fetch_optional(&mut *conn)
                .await
                .map_err(|e| anyhow::anyhow!("pgvector 扩展探测失败: {e}"))?;
        Ok(v)
    }

    let extversion = match probe_version(&mut probe).await? {
        Some(v) => v,
        None => {
            // 未安装 → 尝试自建（托管实例通常无权；失败即报部署前提）
            if let Err(e) = sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
                .execute(&mut probe)
                .await
            {
                anyhow::bail!(
                    "实例未启用 pgvector 扩展（部署前提：pgvector ≥0.8，需 DBA 预先 CREATE EXTENSION vector）：{e}"
                );
            }
            probe_version(&mut probe)
                .await?
                .unwrap_or_else(|| "0.8".to_string())
        }
    };
    if !version_at_least_0_8(&extversion) {
        anyhow::bail!("pgvector 版本 {extversion} 过低，需 ≥0.8（迭代扫描）");
    }

    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .acquire_timeout(Duration::from_secs(config.acquire_timeout_secs))
        .min_connections(0)
        .after_connect(|conn, _| {
            Box::pin(async move {
                // 会话参数：UTC 固定时区（to_char/now() 输出确定）+ HNSW 迭代扫描
                // 与 ef_search（带 scope 过滤的 KNN 尽量取满 k，需 pgvector ≥0.8）。
                // 多语句脚本必须走 raw_sql（prepared 语句不接受多命令）。
                sqlx::raw_sql(
                    "SET timezone = 'UTC'; \
                     SET hnsw.iterative_scan = strict_order; \
                     SET hnsw.ef_search = 80;",
                )
                .execute(conn)
                .await?;
                Ok(())
            })
        })
        .connect(&config.url)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "PostgreSQL 连接池建立失败: {} ({e})",
                host_port_of(&config.url)
            )
        })?;

    super::pg_migration::run_pending(&pool).await?;
    tracing::info!("PG database migrations completed");

    PgStore::ensure_vec_objects(&pool, model_dims).await?;

    // pgvector 的 typmod 即维度（与 SQLite 解析 float[N] 同语义）
    let schema_dims: Option<i32> = sqlx::query_scalar(
        "SELECT atttypmod FROM pg_attribute \
         WHERE attrelid = 'vec_chunks'::regclass AND attname = 'embedding' AND NOT attisdropped",
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| anyhow::anyhow!("vec_chunks 维度探测失败: {e}"))?;

    if let Some(dims) = schema_dims {
        let dims = dims as usize;
        if dims != model_dims {
            anyhow::bail!(
                "Embedding dimension mismatch: model outputs {} dims, but vec_chunks schema has vector({}). \
                 Delete the database to re-run migrations, or configure a compatible model.",
                model_dims,
                dims
            );
        }
    }
    tracing::info!("Embedding dimension validation passed: {} dims", model_dims);

    let store = PgStore::new(pool, config.url.clone());

    let affected = store
        .recover_stuck_processing_documents()
        .await
        .map_err(|e| anyhow::anyhow!("启动恢复失败: {e}"))?;
    if affected > 0 {
        tracing::warn!("Marked {affected} stuck 'processing' document(s) as 'failed'");
    }

    Ok(store)
}

/// 判断 pgvector 版本 ≥0.8（0.x 系列内比较 minor）。
fn version_at_least_0_8(version: &str) -> bool {
    let minor: u32 = version
        .split('.')
        .nth(1)
        .and_then(|s| s.split('-').next().and_then(|p| p.parse().ok()))
        .unwrap_or(0);
    !version.starts_with("0.") || minor >= 8
}

#[cfg(test)]
mod tests {
    use super::*;

    // Covers: 错误信息不得回显含凭据的完整连接串。
    #[test]
    fn host_port_extraction_hides_credentials() {
        let url = "postgres://user:secret@db.example.com:5432/rwiki?sslmode=require";
        assert_eq!(host_port_of(url), "db.example.com:5432");
        assert!(!url.contains("secret") || !host_port_of(url).contains("secret"));
    }

    // Covers: pgvector ≥0.8 版本门槛判定（迭代扫描前提）。
    #[test]
    fn pgvector_version_gate() {
        assert!(version_at_least_0_8("0.8.0"));
        assert!(version_at_least_0_8("0.8"));
        assert!(version_at_least_0_8("1.2.0"));
        assert!(!version_at_least_0_8("0.7.4"));
        assert!(!version_at_least_0_8("0.6"));
    }

    // Covers: scope 谓词 $n 编号渲染正确（Channel 单参、数组 ANY 单参、
    // Published 无参），空集合回退"仅已发布"，占位号推进与片段一致。
    #[test]
    fn pg_scope_fragments_use_numbered_params() {
        let (fragment, param, next) = scope_parts(&RetrievalScope::Channel("c1".into()), 2);
        assert_eq!(fragment, "d.channel_id = $2 AND d.status = 'published'");
        assert!(matches!(param, ScopeParam::Scalar(_)));
        assert_eq!(next, 3);
        let (fragment, param, next) =
            scope_parts(&RetrievalScope::Channels(vec!["a".into(), "b".into()]), 4);
        assert_eq!(
            fragment,
            "d.channel_id = ANY($4) AND d.status = 'published'"
        );
        assert!(matches!(param, ScopeParam::Array(_)));
        assert_eq!(next, 5);
        let (fragment, _, next) = scope_parts(&RetrievalScope::Collection(vec!["x".into()]), 3);
        assert_eq!(fragment, "cm.document_id = ANY($3)");
        assert_eq!(next, 4);
        let (fragment, param, next) = scope_parts(&RetrievalScope::Published, 2);
        assert_eq!(fragment, "d.status = 'published'");
        assert!(matches!(param, ScopeParam::None));
        assert_eq!(next, 2);
        // 空集合回退已发布（与 SQLite 语义一致）
        let (fragment, _, next) = scope_parts(&RetrievalScope::Channels(Vec::new()), 2);
        assert_eq!(fragment, "d.status = 'published'");
        assert_eq!(next, 2);
        let (fragment, _, _) = scope_parts(&RetrievalScope::Collection(Vec::new()), 2);
        assert_eq!(fragment, "d.status = 'published'");
    }
}
