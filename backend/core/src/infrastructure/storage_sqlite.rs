use std::collections::HashMap;
use std::sync::Arc;

use tokio_rusqlite::Connection;
use uuid::Uuid;

use crate::domain::errors::CoreError;

use super::storage::{
    DocumentRecord, FeedbackPage, FeedbackRecord, LowRecallFilter, LowRecallPage,
    LowRecallRecordRow, MetadataStore, NewChunkRow, NewDocument, NewFeedback, NewLowRecallRecord,
    RetrievalStore,
};
use super::vector_store::{tokenize_safe, NeighborChunk, RetrievalScope, SearchResult};

/// SQLite 存储实现。
#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Connection>,
}

impl SqliteStore {
    pub fn new(conn: Arc<Connection>) -> Self {
        Self { conn }
    }

    /// 库内测试与测试工厂取回底层连接做数据预置/校验（不进入跨 crate 公开面）。
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn conn(&self) -> &Arc<Connection> {
        &self.conn
    }
}

/// Convert f32 embedding vector to little-endian bytes for sqlite-vec storage.
fn f32_vec_to_bytes(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Convert sqlite-vec little-endian bytes back to f32 vector.
fn bytes_to_f32_vec(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Split a `\x00`-joined tags column into trimmed non-empty tags.
fn split_tags(tags_text: &str) -> Vec<String> {
    tags_text
        .split('\x00')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Render the RetrievalScope as a SQLite WHERE fragment + bind params.
///
/// 返回 `(WHERE 片段, 绑定参数列表)`，供检索 SQL 拼接：
/// - Channel → `AND d.channel_id = ? AND d.status = 'published'` + channel_id
/// - Channels → `AND d.channel_id IN (?,...) AND d.status = 'published'` + channel_ids
/// - 非空 Collection → `AND cm.document_id IN (?, ...)` + 文档 id 列表
/// - Published / 空 Collection / 空 Channels → `AND d.status = 'published'`（无绑定参数）
///
/// 注意：返回的绑定参数必须在 SQL 中**按片段出现顺序**填入。
fn render_scope(scope: &RetrievalScope) -> (String, Vec<String>) {
    match scope {
        RetrievalScope::Channel(channel_id) => (
            "AND d.channel_id = ? AND d.status = 'published'".to_string(),
            vec![channel_id.clone()],
        ),
        RetrievalScope::Channels(channel_ids) if !channel_ids.is_empty() => {
            let placeholders = (0..channel_ids.len())
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            (
                format!(
                    "AND d.channel_id IN ({}) AND d.status = 'published'",
                    placeholders
                ),
                channel_ids.clone(),
            )
        }
        RetrievalScope::Collection(ids) if !ids.is_empty() => {
            let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
            (
                format!("AND cm.document_id IN ({})", placeholders),
                ids.clone(),
            )
        }
        // Published / 空 Channels / 空 Collection 都回退到"仅已发布"线上行为
        RetrievalScope::Published | RetrievalScope::Channels(_) | RetrievalScope::Collection(_) => {
            ("AND d.status = 'published'".to_string(), Vec::new())
        }
    }
}

fn row_to_search_result(row: &rusqlite::Row<'_>, score: f64) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        chunk_id: row.get(0)?,
        content: row.get(1)?,
        title: row.get(2)?,
        locale: row.get(3)?,
        link: row.get(4)?,
        tags: split_tags(&row.get::<_, String>(5)?),
        section: row.get(6)?,
        document_id: row.get(7)?,
        page_id: row.get(8)?,
        sub_index: row.get(9)?,
        chunk_count: row.get(10)?,
        score,
    })
}

fn row_to_neighbor_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<NeighborChunk> {
    Ok(NeighborChunk {
        chunk_id: row.get(0)?,
        content: row.get(1)?,
        sub_index: row.get(2)?,
        chunk_count: row.get(3)?,
        title: row.get(4)?,
        locale: row.get(5)?,
        link: row.get(6)?,
        tags: split_tags(&row.get::<_, String>(7)?),
        section: row.get(8)?,
        page_id: row.get(9)?,
        document_id: row.get(10)?,
    })
}

/// 按 channel + id 集读取 (id, status)。rusqlite 支持约 900 个绑定参数，
/// 超限回退逐行查询（实践中不会触发）；语义统一为"返回全部命中行"。
/// `&Transaction` 会自动解引用为 `&Connection`，事务内外均可传入。
fn fetch_document_status_rows(
    conn: &rusqlite::Connection,
    channel: &str,
    ids: &[String],
) -> rusqlite::Result<Vec<(String, String)>> {
    if ids.len() <= 900 {
        let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT id, status FROM documents WHERE channel_id = ? AND id IN ({})",
            placeholders
        );

        let params: Vec<Box<dyn rusqlite::ToSql>> =
            std::iter::once(Box::new(channel.to_string()) as Box<dyn rusqlite::ToSql>)
                .chain(
                    ids.iter()
                        .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>),
                )
                .collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    } else {
        Ok(ids
            .iter()
            .filter_map(|id| {
                conn.query_row(
                    "SELECT id, status FROM documents WHERE channel_id = ? AND id = ?",
                    rusqlite::params![channel, id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .ok()
            })
            .collect())
    }
}

/// 批量守卫更新（SQLite 默认 SQLITE_MAX_VARIABLE_NUMBER=999，单臂 >900 个
/// 合法 id 会让 IN 子句参数超限并回滚整个事务，按 900 分块，每块保留状态守卫）。
fn guarded_status_update(
    tx: &rusqlite::Transaction<'_>,
    channel: &str,
    ids: &[Uuid],
    to_status: &str,
    guard_status: &str,
) -> rusqlite::Result<()> {
    const UPDATE_CHUNK: usize = 900;
    for chunk in ids.chunks(UPDATE_CHUNK) {
        let id_strs: Vec<String> = chunk.iter().map(|u| u.to_string()).collect();
        let placeholders = (0..id_strs.len())
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "UPDATE documents SET status = ? WHERE channel_id = ? AND id IN ({}) AND status = ?",
            placeholders
        );
        let params: Vec<Box<dyn rusqlite::ToSql>> =
            std::iter::once(Box::new(to_status.to_string()) as Box<dyn rusqlite::ToSql>)
                .chain(std::iter::once(
                    Box::new(channel.to_string()) as Box<dyn rusqlite::ToSql>
                ))
                .chain(
                    id_strs
                        .iter()
                        .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>),
                )
                .chain(std::iter::once(
                    Box::new(guard_status.to_string()) as Box<dyn rusqlite::ToSql>
                ))
                .collect();
        tx.execute(&sql, rusqlite::params_from_iter(params.iter()))?;
    }
    Ok(())
}

/// Register the sqlite-vec extension globally. Must be called before opening any
/// SQLite connection that uses vec0 virtual tables. Safe to call multiple times.
/// 生产启动链与测试工厂共用的唯一定义点。
pub fn ensure_sqlite_vec_loaded() {
    use std::sync::Once;
    static SQLITE_VEC_INIT: Once = Once::new();
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

impl RetrievalStore for SqliteStore {
    async fn insert_chunks(
        &self,
        document_id: &str,
        rows: Vec<NewChunkRow>,
    ) -> Result<(), CoreError> {
        if rows.is_empty() {
            return Ok(());
        }

        let doc_id = document_id.to_string();

        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                {
                    let mut insert_metadata = tx.prepare(
                        "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                    )?;
                    let mut insert_vec =
                        tx.prepare("INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)")?;
                    let mut insert_fts = tx
                        .prepare("INSERT OR IGNORE INTO fts_chunks(rowid, tokens) VALUES (?, ?)")?;

                    for row in &rows {
                        let tags_text = row.tags.join("\x00");

                        insert_metadata.execute(rusqlite::params![
                            doc_id,
                            row.chunk_id,
                            row.content,
                            row.title,
                            row.locale,
                            row.link,
                            tags_text,
                            row.section,
                            row.page_id,
                            row.sub_index,
                            row.chunk_count,
                            row.content_hash,
                            row.embedding_model,
                        ])?;

                        let rowid = tx.last_insert_rowid();
                        let bytes = f32_vec_to_bytes(&row.embedding);
                        insert_vec.execute(rusqlite::params![rowid, bytes])?;
                        insert_fts.execute(rusqlite::params![rowid, row.fts_tokens])?;
                    }
                }
                tx.commit()?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("插入文档向量失败: {e}")))
    }

    async fn find_cached_embeddings(
        &self,
        content_hashes: &[String],
        model_name: &str,
    ) -> Result<HashMap<String, Vec<f32>>, CoreError> {
        if content_hashes.is_empty() {
            return Ok(HashMap::new());
        }

        let hashes = content_hashes.to_vec();
        let model = model_name.to_string();

        self.conn
            .call(move |conn| {
                // 单条 JOIN 查询同时取回 hash 与缓存向量（与 PG 实现的单查询口径一致）。
                // ORDER BY rowid + HashMap 后写覆盖：同 hash 多行取后写一行
                // chunk_metadata 与 vec_chunks 行总是同事务成对写入，
                // 无向量行的孤儿元数据（脏库）按 INNER JOIN 跳过。
                let placeholders: Vec<String> = hashes.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "SELECT cm.content_hash, v.embedding \
                     FROM chunk_metadata cm \
                     JOIN vec_chunks v ON v.rowid = cm.rowid \
                     WHERE cm.embedding_model = ? AND cm.content_hash IN ({}) \
                     ORDER BY cm.rowid",
                    placeholders.join(",")
                );

                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::types::ToSql> =
                    std::iter::once(&model as &dyn rusqlite::types::ToSql)
                        .chain(hashes.iter().map(|h| h as &dyn rusqlite::types::ToSql))
                        .collect();
                let rows = stmt.query_map(params.as_slice(), |row| {
                    let hash: String = row.get(0)?;
                    let bytes: Vec<u8> = row.get(1)?;
                    Ok((hash, bytes))
                })?;

                let mut result: HashMap<String, Vec<f32>> = HashMap::new();
                for (hash, bytes) in rows.flatten() {
                    result.insert(hash, bytes_to_f32_vec(&bytes));
                }
                Ok::<HashMap<String, Vec<f32>>, rusqlite::Error>(result)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("查找已有向量失败: {e}")))
    }

    async fn backfill_content_hash(&self, model_name: &str) -> Result<u64, CoreError> {
        let model = model_name.to_string();
        let mut total = 0u64;
        loop {
            let model_for_update = model.clone();
            let updated = self
                .conn
                .call(move |conn| {
                    let rows: Vec<(i64, String)> = conn
                        .prepare(
                            "SELECT rowid, content FROM chunk_metadata \
                             WHERE content_hash IS NULL LIMIT 100",
                        )?
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                        .filter_map(|r| r.ok())
                        .collect();

                    if rows.is_empty() {
                        return Ok::<usize, rusqlite::Error>(0);
                    }

                    for (rowid, content) in &rows {
                        let hash = super::vector_store::content_hash(content);
                        conn.execute(
                            "UPDATE chunk_metadata SET content_hash = ?1, embedding_model = ?2 WHERE rowid = ?3",
                            rusqlite::params![hash, model_for_update, rowid],
                        )?;
                    }
                    Ok(rows.len())
                })
                .await
                .map_err(|e| CoreError::DatabaseError(format!("Backfill batch failed: {e}")))?;

            if updated == 0 {
                break;
            }
            total += updated as u64;
            tracing::info!("Backfill progress: processed {} rows", total);
        }
        tracing::info!("Backfill complete: {} rows updated", total);
        Ok(total)
    }

    /// Backfill the fts_chunks FTS5 index for existing chunk_metadata rows.
    /// Idempotent: uses INSERT OR IGNORE to skip already-indexed rows.
    async fn backfill_fts_index(&self) -> Result<(), CoreError> {
        let mut total = 0u64;
        let mut last_rowid: i64 = 0;

        loop {
            let last = last_rowid;
            let batch_size = 1000i64;

            let result = self
                .conn
                .call(move |conn| {
                    let mut stmt = conn.prepare(
                        "SELECT rowid, content FROM chunk_metadata \
                         WHERE rowid > ? ORDER BY rowid LIMIT ?",
                    )?;
                    let rows: Vec<(i64, String)> = stmt
                        .query_map(rusqlite::params![last, batch_size], |row| {
                            Ok((row.get(0)?, row.get(1)?))
                        })?
                        .filter_map(|r| r.ok())
                        .collect();

                    if rows.is_empty() {
                        return Ok::<(usize, i64), rusqlite::Error>((0, last));
                    }

                    let max_rowid = rows.iter().map(|(id, _)| *id).max().unwrap_or(last);

                    for (rowid, content) in &rows {
                        let tokens = tokenize_safe(content);
                        conn.execute(
                            "INSERT OR IGNORE INTO fts_chunks(rowid, tokens) VALUES (?, ?)",
                            rusqlite::params![rowid, tokens],
                        )?;
                    }

                    Ok::<(usize, i64), rusqlite::Error>((rows.len(), max_rowid))
                })
                .await
                .map_err(|e| CoreError::DatabaseError(format!("FTS backfill batch failed: {e}")))?;

            let (processed, max_rowid) = result;

            if processed == 0 {
                break;
            }
            total += processed as u64;
            last_rowid = max_rowid;

            tracing::debug!(
                "FTS backfill progress: {} rows processed, last_rowid={}",
                total,
                last_rowid
            );
        }

        if total > 0 {
            tracing::info!("FTS backfill complete: {} rows processed", total);
        } else {
            tracing::debug!("FTS backfill: no rows to process, fts_chunks already up to date");
        }
        Ok(())
    }

    async fn search_by_vector(
        &self,
        query_vec: &[f64],
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let query_bytes =
            f32_vec_to_bytes(&query_vec.iter().map(|&v| v as f32).collect::<Vec<_>>());
        let top_k_i64 = top_k as i64;

        let (status_filter, collection_ids) = render_scope(scope);

        self.conn
            .call(move |conn| {
                let sql = format!(
                    "SELECT cm.chunk_id, cm.content, cm.title, cm.locale, cm.link, cm.tags, cm.section, \
                            cm.document_id, cm.page_id, cm.sub_index, cm.chunk_count, v.distance \
                     FROM vec_chunks v \
                     LEFT JOIN chunk_metadata cm ON v.rowid = cm.rowid \
                     JOIN documents d ON cm.document_id = d.id \
                     WHERE v.embedding MATCH ? AND k = ? \
                       {}",
                    status_filter
                );

                let mut stmt = conn.prepare(&sql)?;

                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                    Box::new(query_bytes),
                    Box::new(top_k_i64),
                ];
                for id in &collection_ids {
                    params.push(Box::new(id.clone()));
                }

                let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                    // vec0 默认 L2：score = 1 - distance（现状口径）
                    let distance: f32 = row.get(11)?;
                    row_to_search_result(row, 1.0 - distance as f64)
                })?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?);
                }
                Ok::<Vec<SearchResult>, rusqlite::Error>(results)
            })
            .await
            .map_err(|e| CoreError::ProcessingError(format!("搜索失败: {e}")))
    }

    async fn search_by_keyword(
        &self,
        fts_query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let top_k_i64 = top_k as i64;
        let fts_query = fts_query.to_string();

        let (status_filter, collection_ids) = render_scope(scope);

        let result = self
            .conn
            .call(move |conn| {
                let sql = format!(
                    "SELECT cm.chunk_id, cm.content, cm.title, cm.locale, cm.link, cm.tags, cm.section, \
                            cm.document_id, cm.page_id, cm.sub_index, cm.chunk_count, \
                            bm25(fts_chunks) as score \
                     FROM fts_chunks fts \
                     JOIN chunk_metadata cm ON fts.rowid = cm.rowid \
                     JOIN documents d ON cm.document_id = d.id \
                     WHERE fts_chunks MATCH ? \
                       {} \
                     ORDER BY score \
                     LIMIT ?",
                    status_filter
                );

                let mut stmt = match conn.prepare(&sql) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("FTS keyword search prepare failed (FTS index may be missing): {e}");
                        return Ok::<Vec<SearchResult>, rusqlite::Error>(Vec::new());
                    }
                };

                // Build params based on scope. SQL placeholder order is:
                //   MATCH ?  ,  (status_filter: cm.document_id IN (?, ...))  ,  LIMIT ?
                // so the collection ids must be bound BEFORE top_k (which fills LIMIT).
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(fts_query)];
                for id in &collection_ids {
                    params.push(Box::new(id.clone()));
                }
                params.push(Box::new(top_k_i64));

                let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
                    // BM25 returns negative scores; negate for positive ranking.
                    // RRF only needs relative ordering, but consumers expect higher = better.
                    let raw_score: f64 = row.get(11)?;
                    row_to_search_result(row, -raw_score)
                })?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?);
                }
                Ok::<Vec<SearchResult>, rusqlite::Error>(results)
            })
            .await;

        match result {
            Ok(results) => Ok(results),
            Err(e) => {
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
        let pid = page_id.to_string();

        let (status_filter, collection_ids) = render_scope(scope);

        self.conn
            .call(move |conn| {
                let sql = format!(
                    "SELECT chunk_id, content, sub_index, chunk_count, title, locale, link, tags, section, page_id, document_id \
                     FROM chunk_metadata cm \
                     JOIN documents d ON cm.document_id = d.id \
                     WHERE cm.page_id = ? AND cm.sub_index >= ? AND cm.sub_index < ? \
                       AND cm.chunk_count IS NOT NULL \
                       {} \
                     ORDER BY cm.sub_index",
                    status_filter
                );

                let mut stmt = conn.prepare(&sql)?;

                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                    Box::new(pid),
                    Box::new(start_sub),
                    Box::new(end_sub),
                ];
                for id in &collection_ids {
                    params.push(Box::new(id.clone()));
                }

                let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), row_to_neighbor_chunk)?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?);
                }
                Ok::<Vec<NeighborChunk>, rusqlite::Error>(results)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("查询邻居分块失败: {e}")))
    }

    async fn is_empty(&self) -> Result<bool, CoreError> {
        let count: i64 = self
            .conn
            .call(move |conn| {
                let count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM chunk_metadata", [], |row| row.get(0))?;
                Ok::<i64, rusqlite::Error>(count)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("空库检查失败: {e}")))?;
        Ok(count == 0)
    }

    async fn remove_document_chunks(&self, document_id: &str) -> Result<(), CoreError> {
        let doc_id_str = document_id.to_string();

        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                let rowids: Vec<i64> = {
                    let mut stmt =
                        tx.prepare("SELECT rowid FROM chunk_metadata WHERE document_id = ?")?;
                    let rows = stmt.query_map(rusqlite::params![doc_id_str], |row| row.get(0))?;
                    rows.collect::<Result<Vec<_>, _>>()?
                };

                if rowids.is_empty() {
                    // Document has no indexed chunks (may have failed during indexing).
                    // This is a valid state — return Ok to make removal idempotent.
                    tx.commit()?;
                    return Ok::<(), rusqlite::Error>(());
                }

                {
                    // Delete from fts_chunks first (FTS external content table).
                    // FTS5 external-content tables may fail on DELETE if the content table
                    // schema does not exactly match the FTS columns. Tolerate this failure:
                    // orphaned FTS entries are filtered out by the JOIN on chunk_metadata
                    // during search, and backfill_fts_index (INSERT OR IGNORE) handles cleanup.
                    for rowid in &rowids {
                        if let Err(e) = tx.execute(
                            "DELETE FROM fts_chunks WHERE rowid = ?",
                            rusqlite::params![rowid],
                        ) {
                            tracing::debug!(
                                "FTS delete for rowid {} failed (non-critical): {}",
                                rowid,
                                e
                            );
                        }
                    }
                    // Delete from vec_chunks first (foreign data depends on chunk_metadata rowid)
                    for rowid in &rowids {
                        tx.execute(
                            "DELETE FROM vec_chunks WHERE rowid = ?",
                            rusqlite::params![rowid],
                        )?;
                    }
                    tx.execute(
                        "DELETE FROM chunk_metadata WHERE document_id = ?",
                        rusqlite::params![doc_id_str],
                    )?;
                }

                tx.commit()?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("删除文档向量失败: {e}")))
    }
}

impl MetadataStore for SqliteStore {
    async fn ping(&self) -> Result<(), CoreError> {
        self.conn
            .call(|conn| {
                conn.query_row("SELECT 1", [], |_| Ok::<(), rusqlite::Error>(()))?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("健康检查失败: {e}")))
    }

    async fn insert_document(&self, doc: NewDocument) -> Result<(), CoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO documents (id, file_name, status, row_count, channel_id) VALUES (?, ?, 'processing', ?, ?)",
                    rusqlite::params![doc.id, doc.file_name, doc.row_count, doc.channel_id],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("数据库写入失败: {e}")))
    }

    async fn get_document(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<Option<DocumentRecord>, CoreError> {
        let doc_id = document_id.to_string();
        let channel = channel_id.map(str::to_string);

        self.conn
            .call(move |conn| {
                let result = match channel {
                    Some(channel) => conn.query_row(
                        "SELECT id, file_name, status, row_count, channel_id, error_message, created_at \
                         FROM documents WHERE id = ?1 AND channel_id = ?2",
                        rusqlite::params![doc_id, channel],
                        row_to_document_record,
                    ),
                    None => conn.query_row(
                        "SELECT id, file_name, status, row_count, channel_id, error_message, created_at \
                         FROM documents WHERE id = ?1",
                        rusqlite::params![doc_id],
                        row_to_document_record,
                    ),
                };
                Ok::<Option<DocumentRecord>, rusqlite::Error>(result.ok())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("查询文档失败: {e}")))
    }

    async fn list_documents_by_channel(
        &self,
        channel_id: &str,
    ) -> Result<Vec<DocumentRecord>, CoreError> {
        let channel = channel_id.to_string();

        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, file_name, status, row_count, channel_id, error_message, created_at FROM documents WHERE channel_id = ? ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map(rusqlite::params![channel], row_to_document_record)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok::<Vec<DocumentRecord>, rusqlite::Error>(rows)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("获取文档列表失败: {e}")))
    }

    async fn set_document_status(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
        from_status: Option<&str>,
        to_status: &str,
    ) -> Result<u64, CoreError> {
        let doc_id = document_id.to_string();
        let channel = channel_id.map(str::to_string);
        let from = from_status.map(str::to_string);
        let to = to_status.to_string();

        self.conn
            .call(move |conn| {
                // Guarded update: the from-status predicate (when present) keeps
                // concurrent state transitions from overwriting each other.
                let affected = match (&channel, &from) {
                    (Some(channel), Some(from)) => conn.execute(
                        "UPDATE documents SET status = ?1 WHERE id = ?2 AND channel_id = ?3 AND status = ?4",
                        rusqlite::params![to, doc_id, channel, from],
                    )?,
                    (Some(channel), None) => conn.execute(
                        "UPDATE documents SET status = ?1 WHERE id = ?2 AND channel_id = ?3",
                        rusqlite::params![to, doc_id, channel],
                    )?,
                    (None, Some(from)) => conn.execute(
                        "UPDATE documents SET status = ?1 WHERE id = ?2 AND status = ?4",
                        rusqlite::params![to, doc_id, from],
                    )?,
                    (None, None) => conn.execute(
                        "UPDATE documents SET status = ?1 WHERE id = ?2",
                        rusqlite::params![to, doc_id],
                    )?,
                };
                Ok::<usize, rusqlite::Error>(affected)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("更新文档状态失败: {e}")))
            .map(|n| n as u64)
    }

    async fn mark_document_failed(
        &self,
        document_id: &str,
        error_message: &str,
    ) -> Result<(), CoreError> {
        let doc_id = document_id.to_string();
        let err_msg = error_message.to_string();

        self.conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE documents SET status = 'failed', error_message = ?1 WHERE id = ?2",
                    rusqlite::params![err_msg, doc_id],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("更新文档状态失败: {e}")))
    }

    async fn document_statuses(
        &self,
        channel_id: &str,
        ids: &[Uuid],
    ) -> Result<Vec<(String, String)>, CoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let channel = channel_id.to_string();
        let id_strs: Vec<String> = ids.iter().map(|u| u.to_string()).collect();

        self.conn
            .call(move |conn| fetch_document_status_rows(conn, &channel, &id_strs))
            .await
            .map_err(|e| CoreError::DatabaseError(format!("批量状态更新失败: {e}")))
    }

    async fn set_documents_status(
        &self,
        channel_id: &str,
        publish_ids: &[Uuid],
        unpublish_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, bool)>, CoreError> {
        let channel = channel_id.to_string();
        let publish = publish_ids.to_vec();
        let unpublish = unpublish_ids.to_vec();

        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;

                let all_ids: Vec<Uuid> = publish.iter().chain(unpublish.iter()).copied().collect();

                let mut current_statuses = HashMap::new();
                if !all_ids.is_empty() {
                    let id_strs: Vec<String> = all_ids.iter().map(|u| u.to_string()).collect();
                    for (id_str, status) in fetch_document_status_rows(&tx, &channel, &id_strs)? {
                        if let Ok(uuid) = Uuid::parse_str(&id_str) {
                            current_statuses.insert(uuid, status);
                        }
                    }
                }

                let (applied_flags, valid_publish_ids, valid_unpublish_ids) =
                    super::storage::classify_batch_status_guards(
                        &publish,
                        &unpublish,
                        &current_statuses,
                    );

                guarded_status_update(&tx, &channel, &valid_publish_ids, "published", "draft")?;
                guarded_status_update(&tx, &channel, &valid_unpublish_ids, "draft", "published")?;

                tx.commit()?;

                Ok::<Vec<(Uuid, bool)>, rusqlite::Error>(applied_flags)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("批量状态更新失败: {e}")))
    }

    async fn delete_document_row(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<u64, CoreError> {
        let doc_id = document_id.to_string();
        let channel = channel_id.map(str::to_string);

        self.conn
            .call(move |conn| {
                let affected = conn.execute(
                    "DELETE FROM documents WHERE id = ?1 AND channel_id = ?2",
                    rusqlite::params![doc_id, channel],
                )?;
                Ok::<usize, rusqlite::Error>(affected)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("删除文档失败: {e}")))
            .map(|n| n as u64)
    }

    async fn has_published_documents_for_channel(&self, channel_id: &str) -> bool {
        let channel_id = channel_id.to_string();
        self.conn
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM documents WHERE channel_id = ? AND status = 'published'",
                    rusqlite::params![channel_id],
                    |row| row.get(0),
                )?;
                Ok::<bool, rusqlite::Error>(count > 0)
            })
            .await
            .unwrap_or(false)
    }

    async fn has_published_documents_for_channels(&self, channel_ids: &[String]) -> bool {
        if channel_ids.is_empty() {
            return false;
        }
        let channel_ids = channel_ids.to_vec();
        self.conn
            .call(move |conn| {
                let placeholders = (0..channel_ids.len())
                    .map(|_| "?")
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "SELECT COUNT(*) FROM documents \
                     WHERE channel_id IN ({}) AND status = 'published'",
                    placeholders
                );
                let params: Vec<&dyn rusqlite::ToSql> = channel_ids
                    .iter()
                    .map(|id| id as &dyn rusqlite::ToSql)
                    .collect();
                let count: i64 = conn.query_row(&sql, params.as_slice(), |row| row.get(0))?;
                Ok::<bool, rusqlite::Error>(count > 0)
            })
            .await
            .unwrap_or(false)
    }

    async fn count_documents_with_status(&self, status: &str) -> Result<u64, CoreError> {
        let status = status.to_string();
        self.conn
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM documents WHERE status = ?",
                    rusqlite::params![status],
                    |row| row.get(0),
                )?;
                Ok::<i64, rusqlite::Error>(count)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("指标统计失败: {e}")))
            .map(|n| n as u64)
    }

    async fn count_chunks(&self) -> Result<u64, CoreError> {
        self.conn
            .call(move |conn| {
                let count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM chunk_metadata", [], |row| row.get(0))?;
                Ok::<i64, rusqlite::Error>(count)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("指标统计失败: {e}")))
            .map(|n| n as u64)
    }

    async fn recover_stuck_processing_documents(&self) -> Result<u64, CoreError> {
        self.conn
            .call(move |conn| {
                let affected = conn.execute(
                    "UPDATE documents SET status = 'failed' WHERE status = 'processing'",
                    [],
                )?;
                Ok::<usize, rusqlite::Error>(affected)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("启动恢复失败: {e}")))
            .map(|n| n as u64)
    }

    async fn validate_historical_rows_have_channel_id(&self) -> Result<(), CoreError> {
        crate::config::validate_historical_rows_have_channel_id(
            &self.conn,
            &["documents", "chat_feedback", "low_recall_records"],
        )
        .await
        .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    async fn upsert_feedback(&self, feedback: NewFeedback) -> Result<(), CoreError> {
        self.conn
            .call(move |conn| {
                match feedback.feedback {
                    None => {
                        // Idempotent: no error if the row doesn't exist.
                        conn.execute(
                            "DELETE FROM chat_feedback WHERE channel_id = ?1 AND session_id = ?2 AND message_id = ?3",
                            rusqlite::params![feedback.channel_id, feedback.session_id, feedback.message_id],
                        )?;
                    }
                    Some(value) => {
                        conn.execute(
                            "INSERT OR REPLACE INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            rusqlite::params![
                                feedback.channel_id,
                                feedback.session_id,
                                feedback.message_id,
                                value,
                                feedback.user_message,
                                feedback.assistant_message,
                            ],
                        )?;
                    }
                }
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    async fn list_feedback(
        &self,
        channel_id: &str,
        feedback: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<FeedbackPage, CoreError> {
        let channel_id = channel_id.to_string();
        let feedback = feedback.map(str::to_string);

        self.conn
            .call(move |conn| {
                let base_where = "WHERE channel_id = ?1";

                let (count_sql, data_sql) = if feedback.is_some() {
                    (
                        format!("SELECT COUNT(*) FROM chat_feedback {base_where} AND feedback = ?2"),
                        format!(
                            "SELECT id, channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                             FROM chat_feedback {base_where} AND feedback = ?2 \
                             ORDER BY created_at DESC LIMIT ?3 OFFSET ?4"
                        ),
                    )
                } else {
                    (
                        format!("SELECT COUNT(*) FROM chat_feedback {base_where}"),
                        format!(
                            "SELECT id, channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at \
                             FROM chat_feedback {base_where} \
                             ORDER BY created_at DESC LIMIT ?2 OFFSET ?3"
                        ),
                    )
                };

                let total = if let Some(ref feedback) = feedback {
                    conn.query_row(
                        &count_sql,
                        rusqlite::params![channel_id, feedback],
                        |row| row.get::<_, i64>(0),
                    )?
                } else {
                    conn.query_row(&count_sql, rusqlite::params![channel_id], |row| {
                        row.get::<_, i64>(0)
                    })?
                };

                let items = if let Some(ref feedback) = feedback {
                    conn.prepare(&data_sql)?
                        .query_map(
                            rusqlite::params![channel_id, feedback, limit, offset],
                            row_to_feedback_record,
                        )?
                        .collect::<Result<Vec<_>, _>>()?
                } else {
                    conn.prepare(&data_sql)?
                        .query_map(
                            rusqlite::params![channel_id, limit, offset],
                            row_to_feedback_record,
                        )?
                        .collect::<Result<Vec<_>, _>>()?
                };

                Ok::<FeedbackPage, rusqlite::Error>(FeedbackPage { items, total })
            })
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    async fn insert_low_recall_record(&self, record: NewLowRecallRecord) -> Result<(), CoreError> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO low_recall_records \
                     (session_id, channel_id, query, top_score, result_count, sources) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        record.session_id,
                        record.channel_id,
                        record.query,
                        record.top_score,
                        record.result_count,
                        record.sources_json,
                    ],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }

    async fn list_low_recall_records(
        &self,
        filter: LowRecallFilter,
        limit: i64,
        offset: i64,
    ) -> Result<LowRecallPage, CoreError> {
        self.conn
            .call(move |conn| {
                // 动态拼 WHERE 与参数；channel_id 必须放在最前，作为首条过滤条件
                let mut where_clauses: Vec<String> = Vec::new();
                let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                let mut idx = 1usize;

                where_clauses.push(format!("channel_id = ?{}", idx));
                sql_params.push(Box::new(filter.channel_id));
                idx += 1;

                if let Some(min) = filter.min_score {
                    where_clauses.push(format!("top_score >= ?{}", idx));
                    sql_params.push(Box::new(min));
                    idx += 1;
                }
                if let Some(max) = filter.max_score {
                    where_clauses.push(format!("top_score <= ?{}", idx));
                    sql_params.push(Box::new(max));
                    idx += 1;
                }
                if let Some(f) = filter.from_ts {
                    where_clauses.push(format!("created_at >= ?{}", idx));
                    sql_params.push(Box::new(f));
                    idx += 1;
                }
                if let Some(t) = filter.to_ts {
                    where_clauses.push(format!("created_at <= ?{}", idx));
                    sql_params.push(Box::new(t));
                    idx += 1;
                }

                let where_sql = format!("WHERE {}", where_clauses.join(" AND "));

                let count_sql = format!("SELECT COUNT(*) FROM low_recall_records {}", where_sql);
                let total: i64 = {
                    let param_refs: Vec<&dyn rusqlite::ToSql> =
                        sql_params.iter().map(|p| p.as_ref()).collect();
                    conn.query_row(&count_sql, param_refs.as_slice(), |row| row.get(0))?
                };

                let limit_idx = idx;
                let offset_idx = idx + 1;
                let data_sql = format!(
                    "SELECT id, channel_id, session_id, query, top_score, result_count, sources, created_at \
                     FROM low_recall_records {} \
                     ORDER BY created_at DESC LIMIT ?{} OFFSET ?{}",
                    where_sql, limit_idx, offset_idx
                );

                // 完整参数顺序：where 参数 + limit + offset
                let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = sql_params;
                all_params.push(Box::new(limit));
                all_params.push(Box::new(offset));
                let param_refs: Vec<&dyn rusqlite::ToSql> =
                    all_params.iter().map(|p| p.as_ref()).collect();

                let mut stmt = conn.prepare(&data_sql)?;
                let items: Vec<LowRecallRecordRow> = stmt
                    .query_map(param_refs.as_slice(), row_to_low_recall_record)?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok::<LowRecallPage, rusqlite::Error>(LowRecallPage { items, total })
            })
            .await
            .map_err(|e| CoreError::DatabaseError(e.to_string()))
    }
}

fn row_to_document_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocumentRecord> {
    Ok(DocumentRecord {
        id: row.get(0)?,
        file_name: row.get(1)?,
        status: row.get(2)?,
        row_count: row.get(3)?,
        channel_id: row.get(4)?,
        error_message: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn row_to_feedback_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedbackRecord> {
    Ok(FeedbackRecord {
        id: row.get(0)?,
        channel_id: row.get(1)?,
        session_id: row.get(2)?,
        message_id: row.get(3)?,
        feedback: row.get(4)?,
        user_message: row.get(5)?,
        assistant_message: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_to_low_recall_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<LowRecallRecordRow> {
    Ok(LowRecallRecordRow {
        id: row.get(0)?,
        channel_id: row.get(1)?,
        session_id: row.get(2)?,
        query: row.get(3)?,
        top_score: row.get(4)?,
        result_count: row.get(5)?,
        sources_json: row.get(6)?,
        created_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Covers: RetrievalScope::Channel renders the channel + published predicate and binds channel_id.
    #[test]
    fn scope_channel_renders_channel_and_published() {
        let scope = RetrievalScope::Channel("help_center".to_string());
        let (sql, params) = render_scope(&scope);
        assert_eq!(sql, "AND d.channel_id = ? AND d.status = 'published'");
        assert_eq!(params, vec!["help_center".to_string()]);
    }

    // Covers: RetrievalScope::Published keeps the original published-only predicate.
    #[test]
    fn scope_published_renders_published_only() {
        let (sql, params) = render_scope(&RetrievalScope::Published);
        assert_eq!(sql, "AND d.status = 'published'");
        assert!(params.is_empty());
    }

    // Covers: RetrievalScope::Collection binds document ids and skips the status filter.
    #[test]
    fn scope_collection_binds_document_ids() {
        let ids = vec!["doc_a".to_string(), "doc_b".to_string()];
        let scope = RetrievalScope::Collection(ids);
        let (sql, params) = render_scope(&scope);
        assert_eq!(sql, "AND cm.document_id IN (?,?)");
        assert_eq!(params, vec!["doc_a".to_string(), "doc_b".to_string()]);
    }

    // Covers: empty Channels / empty Collection fall back to published-only (线上行为).
    #[test]
    fn scope_empty_variants_fall_back_to_published() {
        let (sql, params) = render_scope(&RetrievalScope::Channels(Vec::new()));
        assert_eq!(sql, "AND d.status = 'published'");
        assert!(params.is_empty());

        let (sql, params) = render_scope(&RetrievalScope::Collection(Vec::new()));
        assert_eq!(sql, "AND d.status = 'published'");
        assert!(params.is_empty());
    }

    // Covers: f32 向量与 sqlite-vec 存储字节序往返无损（缓存向量复用路径的正确性前提）。
    #[test]
    fn f32_bytes_roundtrip_is_lossless() {
        let vec = vec![0.25f32, -1.5, 3.0e-7, 0.0, f32::MIN, f32::MAX];
        let bytes = f32_vec_to_bytes(&vec);
        assert_eq!(bytes.len(), vec.len() * 4);
        assert_eq!(bytes_to_f32_vec(&bytes), vec);
    }
}
