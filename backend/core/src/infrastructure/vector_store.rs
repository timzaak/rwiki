use std::sync::Arc;
use std::sync::OnceLock;

use jieba_rs::Jieba;
use rig::embeddings::{Embedding, EmbeddingModel, EmbeddingsBuilder};
use rig::OneOrMany;
use tokio_rusqlite::Connection;
use uuid::Uuid;

use crate::domain::errors::CoreError;

use super::document_chunk::DocumentChunk;
use super::embedding_model::AppEmbeddingModel;
use super::xlsx_parser::ContentType;

static JIEBA: OnceLock<Jieba> = OnceLock::new();

/// Tokenize text using jieba for FTS5 indexing.
/// Returns space-separated tokens for FTS5 unicode61 tokenizer.
pub(crate) fn tokenize_for_fts(text: &str) -> String {
    let jieba = JIEBA.get_or_init(Jieba::new);
    jieba
        .cut(text, false)
        .iter()
        .map(|t| t.word)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fallback tokenization when jieba fails (e.g. special encoding).
/// Splits on non-alphanumeric characters.
pub(crate) fn tokenize_fallback(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tokenize with jieba, falling back to simple split on panic.
fn tokenize_safe(text: &str) -> String {
    std::panic::catch_unwind(|| tokenize_for_fts(text)).unwrap_or_else(|_| {
        tracing::warn!("jieba tokenize failed, using fallback");
        tokenize_fallback(text)
    })
}

/// Resolve FTS tokens based on document content type.
/// Returns precomputed tokens for OpenAPI documents, falls back to tokenize_safe for others.
pub(crate) fn resolve_fts_tokens(
    content_type: &ContentType,
    fts_tokens: &Option<String>,
    content_text: &str,
) -> String {
    match (content_type, fts_tokens) {
        (ContentType::OpenApi, Some(tokens)) => tokens.clone(),
        _ => tokenize_safe(content_text),
    }
}

/// Sanitize a user query for safe FTS5 MATCH usage.
/// Strips FTS5 operators (AND, OR, NOT) and special characters (quotes, parens, asterisks).
pub(crate) fn sanitize_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|t| !matches!(t.to_uppercase().as_str(), "AND" | "OR" | "NOT"))
        .map(|t| t.replace(['"', '*', '(', ')'], ""))
        .filter(|t| !t.is_empty())
        .filter(|t| !matches!(t.to_uppercase().as_str(), "AND" | "OR" | "NOT"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Manages the sqlite-vec backed vector store for document chunks.
///
/// Wraps a tokio_rusqlite connection with sqlite-vec virtual table for
/// vector similarity search and a metadata table for chunk attributes.
pub struct VectorStoreManager {
    pub(crate) conn: Arc<Connection>,
    embedding_model: AppEmbeddingModel,
    /// Human-readable model name for dedup checks (e.g. "text-embedding-3-small").
    pub model_name: String,
}

/// A single search result from the vector store.
#[derive(Clone)]
pub struct SearchResult {
    pub chunk_id: String,
    pub content: String,
    pub score: f64,
    pub document_id: String,
    pub page_id: String,
    pub sub_index: Option<i64>,
    pub chunk_count: Option<i64>,
    // Metadata fields propagated from DocumentChunk
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    pub tags: Vec<String>,
    pub section: Option<String>,
}

/// A neighbor chunk retrieved by `get_neighbor_chunks()`.
///
/// Carries full metadata needed by `search_with_expansion()` to convert
/// neighbor chunks back into `SearchResult`.
pub struct NeighborChunk {
    pub chunk_id: String,
    pub content: String,
    pub sub_index: Option<i64>,
    pub chunk_count: Option<i64>,
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    pub tags: Vec<String>,
    pub section: Option<String>,
    pub page_id: String,
    pub document_id: String,
}

/// Convert f64 embedding vector to f32 little-endian bytes for sqlite-vec storage.
fn embedding_to_bytes(vec: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        bytes.extend_from_slice(&(v as f32).to_le_bytes());
    }
    bytes
}

/// Compute MD5 hex hash of text content for dedup.
fn content_hash(text: &str) -> String {
    let digest = md5::compute(text.as_bytes());
    format!("{:x}", digest)
}

/// 检索作用域：默认只命中已发布；集合模式限定到指定文档并放开发布限制；
/// Site 模式限定到指定站点的已发布文档。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RetrievalScope {
    /// 现有行为：d.status='published'
    #[default]
    Published,
    /// eval 用：cm.document_id IN (...)，无发布限制
    Collection(Vec<String>),
    /// 站点作用域：仅命中指定站点且 status='published' 的文档
    Site(String),
}

impl RetrievalScope {
    /// 由可选的 document_ids 构造作用域：None 或空 → Published；否则 Collection。
    /// 统一 chat/eval 的"缺省/空维持线上行为"语义，避免两处手写分支漂移。
    pub fn from_document_ids(ids: Option<&Vec<String>>) -> Self {
        match ids {
            Some(ids) if !ids.is_empty() => RetrievalScope::Collection(ids.clone()),
            _ => RetrievalScope::Published,
        }
    }

    /// 返回 `(WHERE 片段, 绑定参数列表)`，供检索 SQL 拼接：
    /// - Site → `AND d.site_id = ? AND d.status = 'published'` + site_id
    /// - 非空 Collection → `AND cm.document_id IN (?, ...)` + 文档 id 列表
    /// - Published / 空 Collection → `AND d.status = 'published'`（无绑定参数）
    ///
    /// 注意：返回的绑定参数必须在 SQL 中**按片段出现顺序**填入。
    /// search_by_keyword 的 `LIMIT ?` 位于该片段**之后**，故 top_k 必须最后压入 params。
    pub fn filter_sql(&self) -> (String, Vec<String>) {
        match self {
            RetrievalScope::Site(site_id) => (
                "AND d.site_id = ? AND d.status = 'published'".to_string(),
                vec![site_id.clone()],
            ),
            RetrievalScope::Collection(ids) if !ids.is_empty() => {
                let placeholders = (0..ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
                (
                    format!("AND cm.document_id IN ({})", placeholders),
                    ids.clone(),
                )
            }
            // Published 或空 Collection 都回退到"仅已发布"线上行为
            RetrievalScope::Published | RetrievalScope::Collection(_) => {
                ("AND d.status = 'published'".to_string(), Vec::new())
            }
        }
    }
}

/// Options controlling embedding dedup behavior during indexing.
#[derive(Default)]
pub struct IndexOptions {
    /// true = force full re-embedding; false = reuse existing vectors (default).
    pub refresh_embed: bool,
}

/// Result of indexing a document: how many chunks were written vs reused.
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Number of chunks actually written to the store.
    pub indexed: usize,
    /// Number of chunks that reused cached embeddings (no re-embedding needed).
    pub reused: usize,
}

/// Existing embedding lookup result for a content_hash.
#[derive(Debug, Clone)]
struct ExistingEmbedding {
    /// Cached embedding bytes (for reuse, avoiding a re-call to the embedding API).
    embedding_bytes: Vec<u8>,
}

impl VectorStoreManager {
    /// Create a new VectorStoreManager with the given sqlite connection and embedding model.
    pub fn ndims(&self) -> usize {
        self.embedding_model.ndims()
    }

    pub fn new(
        conn: Arc<Connection>,
        embedding_model: AppEmbeddingModel,
        model_name: String,
    ) -> Self {
        Self {
            conn,
            embedding_model,
            model_name,
        }
    }

    /// 根据 content_hash 批量查找已有向量（仅缓存，不判断上线状态）。
    /// 排除 content_hash IS NULL 的旧记录，且只返回与当前模型一致的记录。
    /// 注意：若同一 content_hash 匹配多行，任取一行读 bytes 即可（向量相同）。
    async fn find_existing_embeddings(
        &self,
        content_hashes: &[String],
    ) -> Result<std::collections::HashMap<String, ExistingEmbedding>, CoreError> {
        if content_hashes.is_empty() {
            return Ok(std::collections::HashMap::new());
        }

        let model = self.model_name.clone();
        let hashes = content_hashes.to_vec();

        self.conn
            .call(move |conn| {
                // Step 1: 从 chunk_metadata 查找匹配的 rowid（仅取缓存向量）
                let placeholders: Vec<String> = hashes.iter().map(|_| "?".to_string()).collect();
                let sql = format!(
                    "SELECT cm.content_hash, cm.rowid \
                     FROM chunk_metadata cm \
                     WHERE cm.content_hash IN ({}) \
                     AND cm.content_hash IS NOT NULL \
                     AND cm.embedding_model = ? \
                     GROUP BY cm.content_hash",
                    placeholders.join(",")
                );

                let mut stmt = conn.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::types::ToSql> = hashes
                    .iter()
                    .map(|h| h as &dyn rusqlite::types::ToSql)
                    .chain(std::iter::once(&model as &dyn rusqlite::types::ToSql))
                    .collect();
                let rows = stmt.query_map(params.as_slice(), |row| {
                    let hash: String = row.get(0)?;
                    let rowid: i64 = row.get(1)?;
                    Ok((hash, rowid))
                })?;

                let mut rowid_map: std::collections::HashMap<String, i64> =
                    std::collections::HashMap::new();
                for (hash, rowid) in rows.flatten() {
                    rowid_map.insert(hash, rowid);
                }

                // Step 2: 逐行从 vec_chunks 读取 embedding bytes
                let mut result = std::collections::HashMap::new();
                for (hash, rowid) in rowid_map {
                    let bytes: Vec<u8> = conn.query_row(
                        "SELECT embedding FROM vec_chunks WHERE rowid = ?",
                        rusqlite::params![rowid],
                        |row| row.get(0),
                    )?;
                    result.insert(
                        hash,
                        ExistingEmbedding {
                            embedding_bytes: bytes,
                        },
                    );
                }
                Ok::<std::collections::HashMap<String, ExistingEmbedding>, rusqlite::Error>(result)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("查找已有向量失败: {e}")))
    }

    /// Embed document chunks and insert them into sqlite-vec.
    ///
    /// Returns the number of chunks indexed. Delegates to index_document_with_options
    /// with default options (refresh_embed = false).
    pub async fn index_document(
        &self,
        document_id: Uuid,
        chunks: Vec<DocumentChunk>,
    ) -> Result<IndexStats, CoreError> {
        self.index_document_with_options(document_id, chunks, IndexOptions::default())
            .await
    }

    /// Embed document chunks with dedup options and insert them into sqlite-vec.
    ///
    /// When refresh_embed is false, chunks with content matching existing embeddings
    /// (same content_hash and embedding_model) reuse the cached vector bytes instead
    /// of calling the embedding API. Always creates independent chunk entries for
    /// new documents (self-contained deduplication). Returns the number of chunks indexed.
    pub async fn index_document_with_options(
        &self,
        document_id: Uuid,
        chunks: Vec<DocumentChunk>,
        options: IndexOptions,
    ) -> Result<IndexStats, CoreError> {
        // 自包含去重：始终为新文档创建独立 chunk 条目
        //   - 有缓存向量且未强制刷新 → 复用向量写入（reused 计数）
        //   - 其余 → 重新 embedding
        let (reusable, new_chunks) = if !chunks.is_empty() {
            // 计算每个 chunk 的 content_hash（基于 content.join("\n")）
            let hashes: Vec<String> = chunks
                .iter()
                .map(|c| content_hash(&c.content.join("\n")))
                .collect();

            let existing = self.find_existing_embeddings(&hashes).await?;

            let mut reusable = Vec::new();
            let mut new_chunks = Vec::new();
            for (chunk, hash) in chunks.into_iter().zip(hashes) {
                match existing.get(&hash) {
                    // 有缓存向量且未强制刷新 → 复用向量写入
                    Some(info) if !options.refresh_embed => {
                        reusable.push((chunk, hash, info.embedding_bytes.clone()));
                    }
                    _ => {
                        new_chunks.push(chunk);
                    }
                }
            }
            (reusable, new_chunks)
        } else {
            (Vec::new(), Vec::new())
        };

        // indexed = 实际写入数；在 reusable/new_chunks 被 move 进事务前记录
        let indexed = reusable.len() + new_chunks.len();
        // reused = 复用缓存向量的 chunk 数
        let reused = reusable.len();

        // new_chunks 走正常 embedding 流程（按 64 条分批，避免超出 API 限制）
        let embedded: Vec<(DocumentChunk, OneOrMany<Embedding>)> = if !new_chunks.is_empty() {
            let mut results = Vec::with_capacity(new_chunks.len());
            for batch in new_chunks.chunks(64) {
                let batch_results = EmbeddingsBuilder::new(self.embedding_model.clone())
                    .documents(batch.to_vec())
                    .map_err(|e| CoreError::ProcessingError(format!("文档分块嵌入失败: {e}")))?
                    .build()
                    .await
                    .map_err(|e| CoreError::ProcessingError(format!("向量化失败: {e}")))?;
                results.extend(batch_results);
            }
            results
        } else {
            Vec::new()
        };

        let doc_id_str = document_id.to_string();
        let model_name_for_insert = self.model_name.clone();

        // Insert all chunks in a single transaction
        self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;
                {
                    let mut insert_metadata = tx.prepare(
                        "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
                    )?;
                    let mut insert_vec = tx.prepare(
                        "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)"
                    )?;
                    let mut insert_fts = tx.prepare(
                        "INSERT OR IGNORE INTO fts_chunks(rowid, tokens) VALUES (?, ?)"
                    )?;

                    // 插入 reusable 组：直接写入已有 bytes
                    for (chunk, hash, bytes) in &reusable {
                        let content_text = chunk.content.join("\n");
                        let tags_text = chunk.tags.join("\x00");
                        let section_val = chunk.section.as_deref();

                        insert_metadata.execute(rusqlite::params![
                            doc_id_str,
                            chunk.id,
                            content_text,
                            chunk.title,
                            chunk.locale,
                            chunk.link,
                            tags_text,
                            section_val,
                            chunk.page_id,
                            chunk.sub_index.map(|s| s as i64),
                            chunk.chunk_count.map(|c| c as i64),
                            hash,
                            model_name_for_insert,
                        ])?;

                        let rowid = tx.last_insert_rowid();
                        insert_vec.execute(rusqlite::params![rowid, bytes])?;

                        let fts_tokens = resolve_fts_tokens(&chunk.content_type, &chunk.fts_tokens, &content_text);
                        insert_fts.execute(rusqlite::params![rowid, fts_tokens])?;
                    }

                    // 插入 new_chunks 组：走正常 embedding 结果
                    for (chunk, embeddings) in &embedded {
                        let content_text = chunk.content.join("\n");
                        let tags_text = chunk.tags.join("\x00");
                        let section_val = chunk.section.as_deref();
                        let hash = content_hash(&content_text);

                        insert_metadata.execute(rusqlite::params![
                            doc_id_str,
                            chunk.id,
                            content_text,
                            chunk.title,
                            chunk.locale,
                            chunk.link,
                            tags_text,
                            section_val,
                            chunk.page_id,
                            chunk.sub_index.map(|s| s as i64),
                            chunk.chunk_count.map(|c| c as i64),
                            hash,
                            model_name_for_insert,
                        ])?;

                        let rowid = tx.last_insert_rowid();

                        let emb = embeddings.first();
                        let bytes = embedding_to_bytes(&emb.vec);
                        insert_vec.execute(rusqlite::params![rowid, bytes])?;

                        let fts_tokens = resolve_fts_tokens(&chunk.content_type, &chunk.fts_tokens, &content_text);
                        insert_fts.execute(rusqlite::params![rowid, fts_tokens])?;
                    }
                }
                tx.commit()?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("插入文档向量失败: {e}")))?;

        Ok(IndexStats { indexed, reused })
    }

    /// 分批补齐旧数据的 content_hash 和 embedding_model。
    /// 每个批次单独调用 conn.call()，避免长时间锁定连接。
    pub async fn backfill_content_hash(&self) -> Result<(), CoreError> {
        let model = self.model_name.clone();
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
                        let hash = content_hash(content);
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
        Ok(())
    }

    const FTS_BACKFILL_BATCH_SIZE: usize = 1000;

    /// Backfill the fts_chunks FTS5 index for existing chunk_metadata rows.
    /// Idempotent: uses INSERT OR IGNORE to skip already-indexed rows.
    /// Should be called once during application startup after migrations.
    pub async fn backfill_fts_index(&self) -> Result<(), CoreError> {
        let mut total = 0u64;
        let mut last_rowid: i64 = 0;

        loop {
            let last = last_rowid;
            let batch_size = Self::FTS_BACKFILL_BATCH_SIZE as i64;

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

    /// Vector search by pre-computed embedding vector.
    /// Skips embedding generation and is_empty check (caller is responsible).
    pub async fn search_by_vector(
        &self,
        query_vec: &[f64],
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let query_bytes = embedding_to_bytes(query_vec);
        let top_k_i64 = top_k as i64;

        let (status_filter, collection_ids) = scope.filter_sql();

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

                // Build params based on scope
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                    Box::new(query_bytes),
                    Box::new(top_k_i64),
                ];
                for id in &collection_ids {
                    params.push(Box::new(id.clone()));
                }

                let rows = stmt.query_map(
                    rusqlite::params_from_iter(params.iter()),
                    |row| {
                    let chunk_id: String = row.get(0)?;
                    let content_text: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let locale: Option<String> = row.get(3)?;
                    let link: Option<String> = row.get(4)?;
                    let tags_text: String = row.get(5)?;
                    let section: Option<String> = row.get(6)?;
                    let document_id: String = row.get(7)?;
                    let page_id: String = row.get(8)?;
                    let sub_index: Option<i64> = row.get(9)?;
                    let chunk_count: Option<i64> = row.get(10)?;
                    let distance: f32 = row.get(11)?;

                    let content = content_text;
                    let tags: Vec<String> = tags_text
                        .split('\x00')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    Ok(SearchResult {
                        chunk_id,
                        content,
                        score: 1.0 - distance as f64,
                        document_id,
                        page_id,
                        sub_index,
                        chunk_count,
                        title,
                        locale,
                        link,
                        tags,
                        section,
                    })
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

    /// Search by keyword using FTS5 BM25 ranking.
    /// Tokenizes the query with jieba, sanitizes FTS operators, then runs MATCH.
    /// Only returns chunks from published documents (or collection-specified documents).
    /// Returns Ok(empty) if the FTS index is missing or corrupted, degrading gracefully.
    pub async fn search_by_keyword(
        &self,
        query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let tokens = tokenize_for_fts(query);
        let fts_query = sanitize_fts_query(&tokens);

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        let top_k_i64 = top_k as i64;

        let (status_filter, collection_ids) = scope.filter_sql();

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

                let rows = stmt.query_map(
                    rusqlite::params_from_iter(params.iter()),
                    |row| {
                    let chunk_id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let title: String = row.get(2)?;
                    let locale: Option<String> = row.get(3)?;
                    let link: Option<String> = row.get(4)?;
                    let tags_text: String = row.get(5)?;
                    let section: Option<String> = row.get(6)?;
                    let document_id: String = row.get(7)?;
                    let page_id: String = row.get(8)?;
                    let sub_index: Option<i64> = row.get(9)?;
                    let chunk_count: Option<i64> = row.get(10)?;
                    let raw_score: f64 = row.get(11)?;

                    let tags: Vec<String> = tags_text
                        .split('\x00')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    Ok(SearchResult {
                        chunk_id,
                        content,
                        // BM25 returns negative scores; negate for positive ranking.
                        // RRF only needs relative ordering, but consumers expect higher = better.
                        score: -raw_score,
                        document_id,
                        page_id,
                        sub_index,
                        chunk_count,
                        title,
                        locale,
                        link,
                        tags,
                        section,
                    })
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

    /// Search the vector store for similar document chunks.
    ///
    /// Generates a query embedding, then uses sqlite-vec cosine distance search.
    pub async fn search(
        &self,
        query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if self.is_empty().await {
            return Err(CoreError::ProcessingError(
                "知识库为空，请先上传文档".into(),
            ));
        }

        let query_text = query.to_string();

        let embeddings = self
            .embedding_model
            .embed_texts(vec![query_text])
            .await
            .map_err(|e| CoreError::ProcessingError(format!("查询向量化失败: {e}")))?;

        let query_vec = embeddings
            .first()
            .ok_or_else(|| CoreError::ProcessingError("查询向量化返回空结果".into()))?;

        self.search_by_vector(&query_vec.vec, top_k, scope).await
    }

    /// Retrieve neighbor chunks within a sub_index range for a given page_id.
    ///
    /// Returns chunks ordered by sub_index. Only returns rows where chunk_count
    /// is NOT NULL, ensuring old data without chunk metadata degrades gracefully.
    pub async fn get_neighbor_chunks(
        &self,
        page_id: &str,
        start_sub: i64,
        end_sub: i64,
        scope: &RetrievalScope,
    ) -> Result<Vec<NeighborChunk>, CoreError> {
        let pid = page_id.to_string();

        let (status_filter, collection_ids) = scope.filter_sql();

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

                // Build params based on scope
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                    Box::new(pid),
                    Box::new(start_sub),
                    Box::new(end_sub),
                ];
                for id in &collection_ids {
                    params.push(Box::new(id.clone()));
                }

                let rows = stmt.query_map(
                    rusqlite::params_from_iter(params.iter()),
                    |row| {
                        let chunk_id: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        let sub_index: Option<i64> = row.get(2)?;
                        let chunk_count: Option<i64> = row.get(3)?;
                        let title: String = row.get(4)?;
                        let locale: Option<String> = row.get(5)?;
                        let link: Option<String> = row.get(6)?;
                        let tags_text: String = row.get(7)?;
                        let section: Option<String> = row.get(8)?;
                        let page_id: String = row.get(9)?;
                        let document_id: String = row.get(10)?;

                        let tags: Vec<String> = tags_text
                            .split('\x00')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();

                        Ok(NeighborChunk {
                            chunk_id,
                            content,
                            sub_index,
                            chunk_count,
                            title,
                            locale,
                            link,
                            tags,
                            section,
                            page_id,
                            document_id,
                        })
                    },
                )?;

                let mut results = Vec::new();
                for row in rows {
                    results.push(row.map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?);
                }
                Ok::<Vec<NeighborChunk>, rusqlite::Error>(results)
            })
            .await
            .map_err(|e| CoreError::DatabaseError(format!("查询邻居分块失败: {e}")))
    }

    /// Internal: window expansion on seed hits.
    /// Expands seed hits by fetching neighbor chunks, then deduplicates and trims.
    async fn expand_window(
        &self,
        seed_hits: Vec<SearchResult>,
        window_size: usize,
        max_chunks_per_row: usize,
        max_total_context_chunks: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if seed_hits.is_empty() || window_size == 0 {
            return Ok(seed_hits);
        }

        // Track which chunks are seed hits for preferential treatment in trim
        let seed_keys: std::collections::HashSet<(String, Option<i64>)> = seed_hits
            .iter()
            .map(|r| (r.page_id.clone(), r.sub_index))
            .collect();

        // Build a score map per page_id group — highest score wins
        let mut group_best_score: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        for hit in &seed_hits {
            let entry = group_best_score.entry(hit.page_id.clone()).or_insert(0.0);
            if hit.score > *entry {
                *entry = hit.score;
            }
        }

        // Step 2: Expand — for each unique page_id, query neighbors
        let mut unique_page_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for hit in &seed_hits {
            if !hit.page_id.is_empty() {
                unique_page_ids.insert(hit.page_id.clone());
            }
        }

        let mut neighbor_map: std::collections::HashMap<String, Vec<NeighborChunk>> =
            std::collections::HashMap::new();

        for page_id in &unique_page_ids {
            // Collect all sub_indexes from seed hits in this page
            let seed_subs: Vec<i64> = seed_hits
                .iter()
                .filter(|h| h.page_id == *page_id)
                .filter_map(|h| h.sub_index)
                .collect();

            if seed_subs.is_empty() {
                continue; // Old data with no sub_index, skip expansion
            }

            let min_sub = *seed_subs.iter().min().unwrap_or(&0);
            let max_sub = *seed_subs.iter().max().unwrap_or(&0);
            let start = 0i64.max(min_sub - window_size as i64);
            let end = max_sub + window_size as i64 + 1;

            let neighbors = self.get_neighbor_chunks(page_id, start, end, scope).await?;

            neighbor_map.insert(page_id.clone(), neighbors);
        }

        // Step 3: Dedup — merge seed hits + neighbors, dedup by (page_id, sub_index)
        let mut all_chunks = merge_and_dedup(&seed_hits, &neighbor_map, &group_best_score);

        // Step 4: Order — sort by (page_id, sub_index) for original text order
        all_chunks.sort_by(|a, b| (&a.page_id, a.sub_index).cmp(&(&b.page_id, b.sub_index)));

        // Step 5: Trim — enforce budget limits
        let result = trim_to_budget(
            all_chunks,
            &seed_keys,
            max_chunks_per_row,
            max_total_context_chunks,
        );

        Ok(result)
    }

    /// Search with window expansion: search -> expand -> dedup -> order -> trim.
    ///
    /// Returns enriched context chunks that include neighbor chunks around seed hits,
    /// respecting budget limits. Old data (row_index = -1 or chunk_count = NULL) degrades
    /// gracefully to seed-only results.
    pub async fn search_with_expansion(
        &self,
        query: &str,
        top_k: usize,
        window_size: usize,
        max_chunks_per_row: usize,
        max_total_context_chunks: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        let seed_hits = self.search(query, top_k, scope).await?;
        self.expand_window(
            seed_hits,
            window_size,
            max_chunks_per_row,
            max_total_context_chunks,
            scope,
        )
        .await
    }

    /// Multi-query search with RRF fusion and window expansion.
    ///
    /// 1. Batch embed all queries
    /// 2. Search each query independently via search_by_vector
    /// 3. RRF fuse all results
    /// 4. Window expansion on fused seed hits
    #[allow(clippy::too_many_arguments)]
    pub async fn search_multi_query(
        &self,
        queries: &[String],
        top_k_per_query: usize,
        window_size: usize,
        max_chunks_per_row: usize,
        max_total_context_chunks: usize,
        rrf_k: u64,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if queries.is_empty() {
            return Ok(Vec::new());
        }

        // Single query: delegate to search_with_expansion directly
        if queries.len() == 1 {
            return self
                .search_with_expansion(
                    &queries[0],
                    top_k_per_query,
                    window_size,
                    max_chunks_per_row,
                    max_total_context_chunks,
                    scope,
                )
                .await;
        }

        // Step 1: Batch embed all queries
        let all_embeddings = self
            .embedding_model
            .embed_texts(queries.to_vec())
            .await
            .map_err(|e| CoreError::ProcessingError(format!("批量查询向量化失败: {e}")))?;

        // Step 2: Search each query sequentially (sqlite-vec single-connection serialization)
        let mut all_results: Vec<Vec<SearchResult>> = Vec::with_capacity(queries.len());
        for (i, embedding) in all_embeddings.iter().enumerate() {
            match self
                .search_by_vector(&embedding.vec, top_k_per_query, scope)
                .await
            {
                Ok(results) => {
                    tracing::debug!("query {} returned {} results", i, results.len());
                    all_results.push(results);
                }
                Err(e) => {
                    tracing::warn!("query {} search failed: {e}, skipping", i);
                }
            }
        }

        if all_results.is_empty() {
            return Ok(Vec::new());
        }

        // Step 3: RRF fusion
        let fused = rrf_fuse(&all_results, rrf_k, top_k_per_query);

        if fused.is_empty() || window_size == 0 {
            return Ok(fused);
        }

        // Step 4: Window expansion on fused seed hits
        self.expand_window(
            fused,
            window_size,
            max_chunks_per_row,
            max_total_context_chunks,
            scope,
        )
        .await
    }

    /// Check if the vector store has any embeddings.
    pub async fn is_empty(&self) -> bool {
        self.conn
            .call(move |conn| {
                let count: i64 =
                    conn.query_row("SELECT COUNT(*) FROM chunk_metadata", [], |row| row.get(0))?;
                Ok::<bool, rusqlite::Error>(count == 0)
            })
            .await
            .unwrap_or(true)
    }

    /// Check if the vector store has any published documents for a given site.
    pub async fn has_published_documents_for_site(&self, site_id: &str) -> bool {
        let site_id = site_id.to_string();
        self.conn
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM documents WHERE site_id = ? AND status = 'published'",
                    rusqlite::params![site_id],
                    |row| row.get(0),
                )?;
                Ok::<bool, rusqlite::Error>(count > 0)
            })
            .await
            .unwrap_or(false)
    }

    /// Remove all chunks for a document from both vec_chunks and chunk_metadata.
    pub async fn remove_document(&self, document_id: &Uuid) -> Result<(), CoreError> {
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
            .map_err(|e| CoreError::DatabaseError(format!("删除文档向量失败: {e}")))?;

        Ok(())
    }

    /// Hybrid search combining FTS keyword search and vector search via RRF fusion.
    /// Falls back to pure vector search if FTS fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_hybrid(
        &self,
        query: &str,
        top_k: usize,
        window_size: usize,
        max_chunks_per_row: usize,
        max_total_context_chunks: usize,
        rrf_k: u64,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if self.is_empty().await {
            return Err(CoreError::ProcessingError(
                "知识库为空，请先上传文档".into(),
            ));
        }

        let query_text = query.to_string();

        let embeddings = self
            .embedding_model
            .embed_texts(vec![query_text])
            .await
            .map_err(|e| CoreError::ProcessingError(format!("查询向量化失败: {e}")))?;

        let query_vec = embeddings
            .first()
            .ok_or_else(|| CoreError::ProcessingError("查询向量化返回空结果".into()))?
            .vec
            .clone();

        let fts_results = match self.search_by_keyword(query, top_k, scope).await {
            Ok(results) => {
                tracing::debug!(
                    "hybrid search FTS hits: {} for query={:?}",
                    results.len(),
                    query
                );
                for r in &results {
                    tracing::debug!(
                        "  FTS hit: chunk_id={}, score={:.4}, title={:?}",
                        r.chunk_id,
                        r.score,
                        r.title,
                    );
                }
                results
            }
            Err(e) => {
                tracing::warn!("FTS search failed, degrading to vector-only: {e}");
                Vec::new()
            }
        };

        let fts_hit_count = fts_results.len();
        let vec_results = self.search_by_vector(&query_vec, top_k, scope).await?;
        let vector_hit_count = vec_results.len();
        tracing::debug!(
            "hybrid search vector hits: {} for query={:?}",
            vec_results.len(),
            query
        );
        for r in &vec_results {
            tracing::debug!(
                "  vec hit: chunk_id={}, score={:.4}, title={:?}",
                r.chunk_id,
                r.score,
                r.title,
            );
        }

        let fused = rrf_fuse(&[fts_results, vec_results], rrf_k, top_k);
        let current_span = tracing::Span::current();
        current_span.record("fts.hit_count", fts_hit_count);
        current_span.record("vector.hit_count", vector_hit_count);
        current_span.record("rrf.fused_count", fused.len());
        current_span.record("rrf.k", rrf_k);
        tracing::debug!(
            "hybrid search RRF fused: {} results after expansion",
            fused.len()
        );

        if fused.is_empty() || window_size == 0 {
            return Ok(fused);
        }

        self.expand_window(
            fused,
            window_size,
            max_chunks_per_row,
            max_total_context_chunks,
            scope,
        )
        .await
    }

    /// Multi-query hybrid search: keyword + vector per query, RRF fuse all, then window expansion.
    #[allow(clippy::too_many_arguments)]
    pub async fn search_multi_query_hybrid(
        &self,
        queries: &[String],
        top_k_per_query: usize,
        window_size: usize,
        max_chunks_per_row: usize,
        max_total_context_chunks: usize,
        rrf_k: u64,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        if queries.is_empty() {
            return Ok(Vec::new());
        }

        if self.is_empty().await {
            return Err(CoreError::ProcessingError(
                "知识库为空，请先上传文档".into(),
            ));
        }

        if queries.len() == 1 {
            return self
                .search_hybrid(
                    &queries[0],
                    top_k_per_query,
                    window_size,
                    max_chunks_per_row,
                    max_total_context_chunks,
                    rrf_k,
                    scope,
                )
                .await;
        }

        let all_embeddings = self
            .embedding_model
            .embed_texts(queries.to_vec())
            .await
            .map_err(|e| CoreError::ProcessingError(format!("批量查询向量化失败: {e}")))?;

        let mut all_results: Vec<Vec<SearchResult>> = Vec::with_capacity(queries.len() * 2);
        let mut fts_hit_count = 0usize;
        let mut vector_hit_count = 0usize;

        for (i, query) in queries.iter().enumerate() {
            match self.search_by_keyword(query, top_k_per_query, scope).await {
                Ok(fts_results) if !fts_results.is_empty() => {
                    fts_hit_count += fts_results.len();
                    all_results.push(fts_results);
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("FTS search for query {} failed: {e}, skipping", i);
                }
            }

            if let Some(embedding) = all_embeddings.get(i) {
                match self
                    .search_by_vector(&embedding.vec, top_k_per_query, scope)
                    .await
                {
                    Ok(vec_results) => {
                        vector_hit_count += vec_results.len();
                        all_results.push(vec_results);
                    }
                    Err(e) => {
                        tracing::warn!("vector search for query {} failed: {e}", i);
                    }
                }
            }
        }

        if all_results.is_empty() {
            let current_span = tracing::Span::current();
            current_span.record("fts.hit_count", fts_hit_count);
            current_span.record("vector.hit_count", vector_hit_count);
            current_span.record("rrf.fused_count", 0usize);
            current_span.record("rrf.k", rrf_k);
            return Ok(Vec::new());
        }

        let fused = rrf_fuse(&all_results, rrf_k, top_k_per_query);
        let current_span = tracing::Span::current();
        current_span.record("fts.hit_count", fts_hit_count);
        current_span.record("vector.hit_count", vector_hit_count);
        current_span.record("rrf.fused_count", fused.len());
        current_span.record("rrf.k", rrf_k);

        if fused.is_empty() || window_size == 0 {
            return Ok(fused);
        }

        self.expand_window(
            fused,
            window_size,
            max_chunks_per_row,
            max_total_context_chunks,
            scope,
        )
        .await
    }
}

/// Reciprocal Rank Fusion: merge multiple ranked result lists by chunk_id,
/// compute RRF score, and return top-K deduplicated results.
pub(crate) fn rrf_fuse(
    result_lists: &[Vec<SearchResult>],
    k: u64,
    top_k: usize,
) -> Vec<SearchResult> {
    let mut scores: std::collections::HashMap<String, (f64, SearchResult)> =
        std::collections::HashMap::new();

    for results in result_lists {
        for (rank, result) in results.iter().enumerate() {
            let rrf_score = 1.0 / (k as f64 + rank as f64 + 1.0);
            let entry = scores
                .entry(result.chunk_id.clone())
                .or_insert_with(|| (0.0, result.clone()));
            entry.0 += rrf_score;
        }
    }

    let mut ranked: Vec<_> = scores.into_values().collect();
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(top_k);

    ranked
        .into_iter()
        .map(|(score, mut result)| {
            result.score = score;
            result
        })
        .collect()
}

/// Merge seed hits with neighbor chunks, deduplicating by (page_id, sub_index).
///
/// Seed hits take priority over neighbors in case of collision. Neighbor-only chunks
/// receive the highest score from their page_id seed group.
fn merge_and_dedup(
    seed_hits: &[SearchResult],
    neighbor_map: &std::collections::HashMap<String, Vec<NeighborChunk>>,
    group_best_score: &std::collections::HashMap<String, f64>,
) -> Vec<SearchResult> {
    let mut seen: std::collections::HashSet<(String, Option<i64>)> =
        std::collections::HashSet::new();
    let mut result = Vec::new();

    // Add seed hits first (they take priority)
    for hit in seed_hits {
        let key = (hit.page_id.clone(), hit.sub_index);
        if seen.insert(key) {
            result.push(SearchResult {
                chunk_id: hit.chunk_id.clone(),
                content: hit.content.clone(),
                score: hit.score,
                document_id: hit.document_id.clone(),
                page_id: hit.page_id.clone(),
                sub_index: hit.sub_index,
                chunk_count: hit.chunk_count,
                title: hit.title.clone(),
                locale: hit.locale.clone(),
                link: hit.link.clone(),
                tags: hit.tags.clone(),
                section: hit.section.clone(),
            });
        }
    }

    // Add neighbor-only chunks
    for (page_id, neighbors) in neighbor_map {
        let score = group_best_score.get(page_id).copied().unwrap_or(0.0);

        for nb in neighbors {
            let key = (nb.page_id.clone(), nb.sub_index);
            if seen.insert(key) {
                result.push(SearchResult {
                    chunk_id: nb.chunk_id.clone(),
                    content: nb.content.clone(),
                    score,
                    document_id: nb.document_id.clone(),
                    page_id: nb.page_id.clone(),
                    sub_index: nb.sub_index,
                    chunk_count: nb.chunk_count,
                    title: nb.title.clone(),
                    locale: nb.locale.clone(),
                    link: nb.link.clone(),
                    tags: nb.tags.clone(),
                    section: nb.section.clone(),
                });
            }
        }
    }

    result
}

/// Enforce budget limits on the merged chunk list.
///
/// Step 5a: Group by page_id, limit each group to max_chunks_per_row.
///          Prefer seed hits over neighbor-only chunks within each group.
/// Step 5b: If total still exceeds max_total_context_chunks, trim neighbor-only
///          chunks from lowest-scoring seed hit groups first.
fn trim_to_budget(
    chunks: Vec<SearchResult>,
    seed_keys: &std::collections::HashSet<(String, Option<i64>)>,
    max_chunks_per_row: usize,
    max_total_context_chunks: usize,
) -> Vec<SearchResult> {
    // Group by page_id
    let mut groups: std::collections::BTreeMap<String, Vec<SearchResult>> =
        std::collections::BTreeMap::new();
    for chunk in chunks {
        let key = chunk.page_id.clone();
        groups.entry(key).or_default().push(chunk);
    }

    // Step 5a: Limit each group to max_chunks_per_row, preferring seed hits
    let mut trimmed_groups: Vec<Vec<SearchResult>> = Vec::new();
    for (_, mut group_chunks) in groups {
        // Sort within group: seed hits first, then by sub_index
        group_chunks.sort_by(|a, b| {
            let a_is_seed = seed_keys.contains(&(a.page_id.clone(), a.sub_index));
            let b_is_seed = seed_keys.contains(&(b.page_id.clone(), b.sub_index));
            match (a_is_seed, b_is_seed) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.sub_index.cmp(&b.sub_index),
            }
        });
        group_chunks.truncate(max_chunks_per_row);
        trimmed_groups.push(group_chunks);
    }

    // Step 5b: If total exceeds max_total_context_chunks, trim neighbor-only chunks
    // from lowest-scoring seed hit groups first.
    // Calculate total and sort groups by their highest seed hit score (descending).
    let total: usize = trimmed_groups.iter().map(|g| g.len()).sum();
    if total <= max_total_context_chunks {
        let mut result: Vec<SearchResult> = trimmed_groups.into_iter().flatten().collect();
        // Re-sort by (page_id, sub_index) to preserve original order
        result.sort_by(|a, b| (&a.page_id, a.sub_index).cmp(&(&b.page_id, b.sub_index)));
        return result;
    }

    // Score each group by its highest seed hit score
    let mut scored_groups: Vec<(f64, Vec<SearchResult>)> = trimmed_groups
        .into_iter()
        .map(|g| {
            let max_seed_score = g
                .iter()
                .filter(|c| seed_keys.contains(&(c.page_id.clone(), c.sub_index)))
                .map(|c| c.score)
                .fold(f64::NEG_INFINITY, f64::max);
            (max_seed_score, g)
        })
        .collect();

    // Sort groups by score descending (highest score first)
    scored_groups.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Greedily take groups, removing neighbor-only chunks from tail groups if needed
    let mut result: Vec<SearchResult> = Vec::new();
    let mut budget_remaining = max_total_context_chunks;

    for (score, mut group) in scored_groups {
        if budget_remaining == 0 {
            break;
        }

        if group.len() <= budget_remaining {
            // Whole group fits
            budget_remaining -= group.len();
            result.append(&mut group);
        } else {
            // Partial group: prefer seed hits, then fill with neighbors
            let mut seeds = Vec::new();
            let mut neighbors = Vec::new();
            for chunk in group {
                if seed_keys.contains(&(chunk.page_id.clone(), chunk.sub_index)) {
                    seeds.push(chunk);
                } else {
                    neighbors.push(chunk);
                }
            }

            // Take as many seeds as possible
            let seed_take = seeds.len().min(budget_remaining);
            for chunk in seeds.into_iter().take(seed_take) {
                result.push(chunk);
                budget_remaining -= 1;
            }

            // Fill remaining budget with neighbors
            let neighbor_take = neighbors.len().min(budget_remaining);
            for chunk in neighbors.into_iter().take(neighbor_take) {
                result.push(chunk);
                budget_remaining -= 1;
            }

            // If this group (with highest remaining score) doesn't fully fit, lower groups won't either
            let _ = score; // score is used for ordering only
        }
    }

    // Re-sort by (page_id, sub_index) to preserve original text order
    result.sort_by(|a, b| (&a.page_id, a.sub_index).cmp(&(&b.page_id, b.sub_index)));

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::client::EmbeddingsClient;
    use std::sync::Once;

    static SQLITE_VEC_INIT: Once = Once::new();
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
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        });
    }

    /// Create an in-memory SQLite connection with WAL, migrations, and sqlite-vec loaded.
    fn make_sqlite_conn() -> Arc<tokio_rusqlite::Connection> {
        ensure_sqlite_vec_loaded();

        let mut conn =
            rusqlite::Connection::open_in_memory().expect("in-memory SQLite should open");
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .expect("WAL mode should be settable");
        super::super::migration::migrations(1536) // matches text-embedding-3-small default
            .to_latest(&mut conn)
            .expect("migrations should run");

        Arc::new(tokio_rusqlite::Connection::from(conn))
    }

    /// Build a VectorStoreManager over a fresh in-memory SQLite store using the given
    /// embedding model and the shared test model name ("test-dummy").
    fn make_store(embedding_model: AppEmbeddingModel) -> VectorStoreManager {
        VectorStoreManager::new(
            make_sqlite_conn(),
            embedding_model,
            "test-dummy".to_string(),
        )
    }

    /// Create a VectorStoreManager backed by in-memory SQLite with all migrations
    /// and sqlite-vec extension, but without a reachable embedding model.
    /// Uses a dummy OpenAI client — the embedding model is never called by the
    /// SQL-only tests (get_neighbor_chunks / search_by_keyword are pure SQL).
    fn make_sql_only_store() -> VectorStoreManager {
        // Dummy OpenAI embedding model — never invoked by get_neighbor_chunks tests.
        let client = rig::providers::openai::Client::builder()
            .api_key("test-key-unused")
            .build()
            .expect("dummy OpenAI client should build without network");
        make_store(AppEmbeddingModel::new(
            client.embedding_model("text-embedding-3-small"),
        ))
    }

    /// Like `make_sql_only_store`, but point the OpenAI embedding client at `base_url`,
    /// so tests exercising the re-embedding path can mock `/embeddings` offline (mockito).
    fn make_store_with_embedding_base_url(base_url: &str) -> VectorStoreManager {
        let client = rig::providers::openai::Client::builder()
            .api_key("test-key-unused")
            .base_url(base_url)
            .build()
            .expect("OpenAI client should build");
        make_store(AppEmbeddingModel::new(
            client.embedding_model("text-embedding-3-small"),
        ))
    }

    /// Insert a test document row into the documents table.
    async fn insert_test_document(store: &VectorStoreManager, document_id: &str, status: &str) {
        insert_test_document_for_site(store, document_id, status, None).await;
    }

    /// Insert a test document row with an optional site_id.
    async fn insert_test_document_for_site(
        store: &VectorStoreManager,
        document_id: &str,
        status: &str,
        site_id: Option<&str>,
    ) {
        let doc_id = document_id.to_string();
        let status_val = status.to_string();
        let site_id_val = site_id.map(|s| s.to_string());
        store
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO documents (id, file_name, status, row_count, site_id) VALUES (?, 'test.xlsx', ?, ?, ?)",
                    rusqlite::params![doc_id, status_val, 1, site_id_val],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("insert_test_document_for_site should succeed");
    }

    /// Insert a test chunk directly into chunk_metadata + vec_chunks (zero vector matching migration dimensions).
    async fn insert_test_chunk(
        store: &VectorStoreManager,
        document_id: &str,
        chunk_id: &str,
        content: &str,
        page_id: &str,
        sub_index: Option<i64>,
        chunk_count: Option<i64>,
    ) {
        let doc_id = document_id.to_string();
        let cid = chunk_id.to_string();
        let c = content.to_string();
        let pid = page_id.to_string();
        let sub = sub_index;
        let cc = chunk_count;
        let ndims = store.ndims();

        store
            .conn
            .call(move |conn| {
                let dummy_embedding = vec![0u8; ndims * 4];

                conn.execute(
                    "INSERT INTO chunk_metadata (document_id, chunk_id, content, title, locale, link, tags, section, page_id, sub_index, chunk_count, content_hash, embedding_model) \
                     VALUES (?, ?, ?, '', NULL, NULL, '', NULL, ?, ?, ?, NULL, NULL)",
                    rusqlite::params![doc_id, cid, c, pid, sub, cc],
                )?;

                let rowid = conn.last_insert_rowid();
                conn.execute(
                    "INSERT INTO vec_chunks (rowid, embedding) VALUES (?, ?)",
                    rusqlite::params![rowid, dummy_embedding],
                )?;

                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("insert_test_chunk should succeed");
    }

    // Covers: get_neighbor_chunks returns chunks in the correct sub_index range,
    //         ordered by sub_index ascending.
    #[tokio::test]
    async fn get_neighbor_chunks_returns_correct_range() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_a", "published").await;
        insert_test_chunk(
            &store,
            "doc_a",
            "chunk_0",
            "content 0",
            "page_5",
            Some(0),
            Some(3),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_a",
            "chunk_1",
            "content 1",
            "page_5",
            Some(1),
            Some(3),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_a",
            "chunk_2",
            "content 2",
            "page_5",
            Some(2),
            Some(3),
        )
        .await;

        let results = store
            .get_neighbor_chunks("page_5", 0, 3, &RetrievalScope::Published)
            .await
            .expect("get_neighbor_chunks should succeed");

        assert_eq!(results.len(), 3, "should return 3 chunks in range [0,3)");
        assert_eq!(results[0].chunk_id, "chunk_0");
        assert_eq!(results[1].chunk_id, "chunk_1");
        assert_eq!(results[2].chunk_id, "chunk_2");
        assert_eq!(results[0].sub_index, Some(0));
        assert_eq!(results[1].sub_index, Some(1));
        assert_eq!(results[2].sub_index, Some(2));
    }

    // Covers: Chunks with chunk_count=NULL are excluded, ensuring old data degrades gracefully.
    #[tokio::test]
    async fn get_neighbor_chunks_filters_old_data() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_old", "published").await;
        insert_test_chunk(
            &store,
            "doc_old",
            "chunk_old",
            "old content",
            "page_old",
            Some(0),
            None,
        )
        .await;

        let results = store
            .get_neighbor_chunks("page_old", 0, 1, &RetrievalScope::Published)
            .await
            .expect("get_neighbor_chunks should succeed");

        assert!(
            results.is_empty(),
            "chunk with chunk_count=NULL should be filtered out"
        );
    }

    // Covers: get_neighbor_chunks only returns chunks for the specified page_id.
    #[tokio::test]
    async fn get_neighbor_chunks_does_not_cross_page_id() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_A", "published").await;
        insert_test_document(&store, "doc_B", "published").await;
        insert_test_chunk(
            &store,
            "doc_A",
            "a_chunk_0",
            "doc A content 0",
            "page_A",
            Some(0),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_A",
            "a_chunk_1",
            "doc A content 1",
            "page_A",
            Some(1),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_B",
            "b_chunk_0",
            "doc B content 0",
            "page_B",
            Some(0),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_B",
            "b_chunk_1",
            "doc B content 1",
            "page_B",
            Some(1),
            Some(2),
        )
        .await;

        let results = store
            .get_neighbor_chunks("page_A", 0, 2, &RetrievalScope::Published)
            .await
            .expect("get_neighbor_chunks should succeed");

        assert_eq!(results.len(), 2, "should return only page_A chunks");
        assert!(
            results.iter().all(|r| r.page_id == "page_A"),
            "all results should have page_id = page_A"
        );
        assert_eq!(results[0].chunk_id, "a_chunk_0");
        assert_eq!(results[1].chunk_id, "a_chunk_1");
    }

    // Covers: get_neighbor_chunks only returns chunks for the specified page_id.
    #[tokio::test]
    async fn get_neighbor_chunks_does_not_cross_row_index() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_X", "published").await;
        insert_test_chunk(
            &store,
            "doc_X",
            "r0_chunk_0",
            "row 0 content 0",
            "page_0",
            Some(0),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_X",
            "r0_chunk_1",
            "row 0 content 1",
            "page_0",
            Some(1),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_X",
            "r1_chunk_0",
            "row 1 content 0",
            "page_1",
            Some(0),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_X",
            "r1_chunk_1",
            "row 1 content 1",
            "page_1",
            Some(1),
            Some(2),
        )
        .await;

        let results = store
            .get_neighbor_chunks("page_0", 0, 2, &RetrievalScope::Published)
            .await
            .expect("get_neighbor_chunks should succeed");

        assert_eq!(results.len(), 2, "should return only page_0 chunks");
        assert!(
            results.iter().all(|r| r.page_id == "page_0"),
            "all results should have page_id = page_0"
        );
        assert_eq!(results[0].chunk_id, "r0_chunk_0");
        assert_eq!(results[1].chunk_id, "r0_chunk_1");
    }

    // Covers: RetrievalScope::Site generates the correct SQL predicate and binds site_id.
    #[test]
    fn retrieval_scope_site_filter_sql_includes_site_and_published() {
        let scope = RetrievalScope::Site("help_center".to_string());
        let (sql, params) = scope.filter_sql();
        assert_eq!(sql, "AND d.site_id = ? AND d.status = 'published'");
        assert_eq!(params, vec!["help_center".to_string()]);
    }

    // Covers: RetrievalScope::Published keeps the original published-only predicate.
    #[test]
    fn retrieval_scope_published_filter_sql_unchanged() {
        let (sql, params) = RetrievalScope::Published.filter_sql();
        assert_eq!(sql, "AND d.status = 'published'");
        assert!(params.is_empty());
    }

    // Covers: RetrievalScope::Collection binds document ids and skips status filter.
    #[test]
    fn retrieval_scope_collection_filter_sql_uses_document_ids() {
        let ids = vec!["doc_a".to_string(), "doc_b".to_string()];
        let (sql, params) = RetrievalScope::Collection(ids).filter_sql();
        assert_eq!(sql, "AND cm.document_id IN (?,?)");
        assert_eq!(params, vec!["doc_a".to_string(), "doc_b".to_string()]);
    }

    // Covers: has_published_documents_for_site returns true only when the site has published docs.
    #[tokio::test]
    async fn has_published_documents_for_site_reflects_site_scope() {
        let store = make_sql_only_store();

        insert_test_document_for_site(&store, "doc_help_pub", "published", Some("help_center"))
            .await;
        insert_test_document_for_site(&store, "doc_help_draft", "draft", Some("help_center")).await;
        insert_test_document_for_site(&store, "doc_dev_pub", "published", Some("dev_docs")).await;

        assert!(
            store.has_published_documents_for_site("help_center").await,
            "help_center should have published documents"
        );
        assert!(
            store.has_published_documents_for_site("dev_docs").await,
            "dev_docs should have published documents"
        );
        assert!(
            !store.has_published_documents_for_site("unknown").await,
            "unknown site should have no published documents"
        );
    }

    // --- Pipeline unit tests (merge_and_dedup, trim_to_budget) ---

    fn make_search_result(
        doc_id: &str,
        page_id: &str,
        sub_index: Option<i64>,
        score: f64,
        chunk_id: &str,
    ) -> SearchResult {
        SearchResult {
            chunk_id: chunk_id.to_string(),
            content: format!("content for {}", chunk_id),
            score,
            document_id: doc_id.to_string(),
            page_id: page_id.to_string(),
            sub_index,
            chunk_count: Some(3),
            title: String::new(),
            locale: None,
            link: None,
            tags: Vec::new(),
            section: None,
        }
    }

    fn make_seed_keys(
        chunks: &[&SearchResult],
    ) -> std::collections::HashSet<(String, Option<i64>)> {
        chunks
            .iter()
            .map(|c| (c.page_id.clone(), c.sub_index))
            .collect()
    }

    // Covers: merge_and_dedup deduplicates seed hits and neighbors by (page_id, sub_index),
    //         keeping seed hit's score when collision occurs.
    #[test]
    fn merge_and_dedup_prefers_seed_hit_on_collision() {
        let seed = make_search_result("doc_a", "page_5", Some(1), 0.9, "seed_1");
        let neighbor = NeighborChunk {
            chunk_id: "neighbor_1".to_string(),
            content: "neighbor content".to_string(),
            sub_index: Some(1),
            chunk_count: Some(3),
            title: String::new(),
            locale: None,
            link: None,
            tags: Vec::new(),
            section: None,
            page_id: "page_5".to_string(),
            document_id: "doc_a".to_string(),
        };

        let neighbor_map: std::collections::HashMap<String, Vec<NeighborChunk>> = {
            let mut m = std::collections::HashMap::new();
            m.insert("page_5".to_string(), vec![neighbor]);
            m
        };

        let group_best_score: std::collections::HashMap<String, f64> = {
            let mut m = std::collections::HashMap::new();
            m.insert("page_5".to_string(), 0.9);
            m
        };

        let result = merge_and_dedup(&[seed], &neighbor_map, &group_best_score);

        // Should dedup to 1 chunk (seed hit takes priority)
        assert_eq!(result.len(), 1, "collision should dedup to 1");
        assert_eq!(
            result[0].chunk_id, "seed_1",
            "seed hit should win over neighbor"
        );
        assert!(
            (result[0].score - 0.9).abs() < 1e-6,
            "seed hit score preserved"
        );
    }

    // Covers: merge_and_dedup adds neighbor-only chunks with the group's best seed score.
    #[test]
    fn merge_and_dedup_adds_neighbors_with_group_score() {
        let seed = make_search_result("doc_a", "page_5", Some(1), 0.8, "seed_1");
        let neighbor = NeighborChunk {
            chunk_id: "neighbor_0".to_string(),
            content: "neighbor content 0".to_string(),
            sub_index: Some(0),
            chunk_count: Some(3),
            title: String::new(),
            locale: None,
            link: None,
            tags: Vec::new(),
            section: None,
            page_id: "page_5".to_string(),
            document_id: "doc_a".to_string(),
        };

        let neighbor_map: std::collections::HashMap<String, Vec<NeighborChunk>> = {
            let mut m = std::collections::HashMap::new();
            m.insert("page_5".to_string(), vec![neighbor]);
            m
        };

        let group_best_score: std::collections::HashMap<String, f64> = {
            let mut m = std::collections::HashMap::new();
            m.insert("page_5".to_string(), 0.8);
            m
        };

        let result = merge_and_dedup(&[seed], &neighbor_map, &group_best_score);

        assert_eq!(result.len(), 2, "should have seed + neighbor");
        // Neighbor gets the group's best score
        let neighbor_result = result.iter().find(|r| r.chunk_id == "neighbor_0").unwrap();
        assert!(
            (neighbor_result.score - 0.8).abs() < 1e-6,
            "neighbor should get group best score 0.8"
        );
    }

    // Covers: trim_to_budget respects max_total_context_chunks by removing neighbor-only
    //         chunks from lowest-scoring groups first.
    #[test]
    fn trim_to_budget_respects_max_total() {
        // Group 1: doc_a, page_a — high score seed + 2 neighbors = 3 chunks
        let seed_a = make_search_result("doc_a", "page_a", Some(1), 0.9, "seed_a");
        let nb_a0 = make_search_result("doc_a", "page_a", Some(0), 0.9, "nb_a0");
        let nb_a2 = make_search_result("doc_a", "page_a", Some(2), 0.9, "nb_a2");

        // Group 2: doc_b, page_b — low score seed + 2 neighbors = 3 chunks
        let seed_b = make_search_result("doc_b", "page_b", Some(1), 0.5, "seed_b");
        let nb_b0 = make_search_result("doc_b", "page_b", Some(0), 0.5, "nb_b0");
        let nb_b2 = make_search_result("doc_b", "page_b", Some(2), 0.5, "nb_b2");

        let chunks = vec![seed_a, nb_a0, nb_a2, seed_b, nb_b0, nb_b2];
        let seed_keys = make_seed_keys(&[
            &chunks[0], // seed_a
            &chunks[3], // seed_b
        ]);

        // Max total = 4, so 2 chunks should be trimmed (from lowest-scoring group first)
        let result = trim_to_budget(chunks, &seed_keys, 3, 4);

        assert_eq!(result.len(), 4, "should respect max_total=4");

        // doc_a (high score) should keep all 3 chunks
        let doc_a_count = result.iter().filter(|r| r.document_id == "doc_a").count();
        assert_eq!(doc_a_count, 3, "high-score group should keep all chunks");

        // doc_b (low score) should keep only its seed (neighbors trimmed)
        let doc_b_results: Vec<_> = result.iter().filter(|r| r.document_id == "doc_b").collect();
        assert_eq!(
            doc_b_results.len(),
            1,
            "low-score group should be trimmed to 1"
        );
        assert_eq!(
            doc_b_results[0].chunk_id, "seed_b",
            "seed should survive trim"
        );
    }

    // Covers: trim_to_budget respects max_chunks_per_row within each group.
    #[test]
    fn trim_to_budget_respects_max_per_row() {
        let seed = make_search_result("doc_a", "page_a", Some(1), 0.9, "seed");
        let nb0 = make_search_result("doc_a", "page_a", Some(0), 0.9, "nb0");
        let nb2 = make_search_result("doc_a", "page_a", Some(2), 0.9, "nb2");

        let chunks = vec![seed, nb0, nb2];
        let seed_keys = make_seed_keys(&[&chunks[0]]);

        // max_chunks_per_row = 2, should trim 1 neighbor
        let result = trim_to_budget(chunks, &seed_keys, 2, 10);

        assert_eq!(result.len(), 2, "should respect max_chunks_per_row=2");
        // Seed should survive
        assert!(
            result.iter().any(|r| r.chunk_id == "seed"),
            "seed should survive per-row trim"
        );
    }

    // Covers: Old data (empty page_id) degrades to seed-only — no expansion attempted.
    #[test]
    fn trim_to_budget_handles_old_data_gracefully() {
        let old_seed = make_search_result("doc_old", "", None, 0.7, "old_seed");

        let chunks = vec![old_seed];
        let seed_keys = make_seed_keys(&[&chunks[0]]);

        let result = trim_to_budget(chunks, &seed_keys, 3, 12);

        assert_eq!(result.len(), 1, "old data should pass through as seed-only");
        assert_eq!(result[0].chunk_id, "old_seed");
    }

    // --- rrf_fuse tests ---

    fn make_rrf_result(chunk_id: &str, page_id: &str, score: f64) -> SearchResult {
        SearchResult {
            chunk_id: chunk_id.to_string(),
            content: format!("content for {chunk_id}"),
            score,
            document_id: "doc".to_string(),
            page_id: page_id.to_string(),
            sub_index: Some(0),
            chunk_count: Some(3),
            title: String::new(),
            locale: None,
            link: None,
            tags: Vec::new(),
            section: None,
        }
    }

    #[test]
    fn rrf_fuse_single_list_preserves_order() {
        let list = vec![
            make_rrf_result("a", "p1", 0.9),
            make_rrf_result("b", "p1", 0.8),
            make_rrf_result("c", "p1", 0.7),
        ];
        let result = rrf_fuse(&[list], 60, 10);
        assert_eq!(result.len(), 3, "single list should return all items");
        assert_eq!(result[0].chunk_id, "a", "rank 0 should be first");
        assert_eq!(result[1].chunk_id, "b", "rank 1 should be second");
        assert_eq!(result[2].chunk_id, "c", "rank 2 should be third");
    }

    #[test]
    fn rrf_fuse_two_lists_dedup_by_chunk_id() {
        let list1 = vec![
            make_rrf_result("a", "p1", 0.9),
            make_rrf_result("b", "p1", 0.8),
        ];
        let list2 = vec![
            make_rrf_result("b", "p1", 0.95), // overlap with list1
            make_rrf_result("c", "p2", 0.7),
        ];
        let result = rrf_fuse(&[list1, list2], 60, 10);
        assert_eq!(result.len(), 3, "should dedup 'b' to 3 unique items");

        // 'b' appears in list1 at rank 1 and list2 at rank 0, should have highest RRF score
        assert_eq!(
            result[0].chunk_id, "b",
            "overlapping item should rank first"
        );

        // Verify RRF score calculation for 'b': 1/(60+1+1) + 1/(60+0+1) = 1/62 + 1/61
        let expected_b_score = 1.0 / 62.0 + 1.0 / 61.0;
        assert!(
            (result[0].score - expected_b_score).abs() < 1e-10,
            "RRF score for 'b' should be 1/62 + 1/61, got {}",
            result[0].score
        );
    }

    #[test]
    fn rrf_fuse_empty_input_returns_empty() {
        let result = rrf_fuse(&[], 60, 10);
        assert!(result.is_empty(), "empty input should return empty");
    }

    #[test]
    fn rrf_fuse_empty_lists_return_empty() {
        let result = rrf_fuse(&[vec![], vec![]], 60, 10);
        assert!(result.is_empty(), "empty lists should return empty");
    }

    #[test]
    fn rrf_fuse_truncates_to_top_k() {
        let list = vec![
            make_rrf_result("a", "p1", 0.9),
            make_rrf_result("b", "p1", 0.8),
            make_rrf_result("c", "p2", 0.7),
            make_rrf_result("d", "p2", 0.6),
        ];
        let result = rrf_fuse(&[list], 60, 2);
        assert_eq!(result.len(), 2, "should truncate to top_k=2");
        assert_eq!(result[0].chunk_id, "a");
        assert_eq!(result[1].chunk_id, "b");
    }

    #[test]
    fn rrf_fuse_updates_score_field() {
        let list1 = vec![make_rrf_result("a", "p1", 0.9)];
        let list2 = vec![make_rrf_result("a", "p1", 0.8)];
        let result = rrf_fuse(&[list1, list2], 60, 10);
        assert_eq!(result.len(), 1);
        // Score should be 1/(60+0+1) + 1/(60+0+1) = 2/61
        let expected = 2.0 / 61.0;
        assert!(
            (result[0].score - expected).abs() < 1e-10,
            "score field should be updated to RRF score"
        );
    }

    // --- Tokenization and FTS utility tests ---

    #[test]
    fn tokenize_for_fts_chinese_text() {
        let result = super::tokenize_for_fts("内存管理");
        let tokens: Vec<&str> = result.split_whitespace().collect();
        assert!(
            tokens.contains(&"内存"),
            "should contain '内存' token, got: {:?}",
            tokens
        );
        assert!(
            tokens.contains(&"管理"),
            "should contain '管理' token, got: {:?}",
            tokens
        );
    }

    #[test]
    fn tokenize_for_fts_mixed_chinese_english() {
        let result = super::tokenize_for_fts("Rust 语言的内存管理");
        let tokens: Vec<&str> = result.split_whitespace().collect();
        assert!(
            tokens.contains(&"Rust"),
            "should contain 'Rust', got: {:?}",
            tokens
        );
        assert!(
            tokens.contains(&"内存"),
            "should contain '内存', got: {:?}",
            tokens
        );
    }

    #[test]
    fn tokenize_fallback_splits_on_non_alphanumeric() {
        let result = super::tokenize_fallback("hello, world! 123");
        assert_eq!(result, "hello world 123");
    }

    #[test]
    fn sanitize_fts_query_strips_operators_and_special_chars() {
        let result = super::sanitize_fts_query("test AND query OR (\"phrase\") NOT *bad*");
        assert!(!result.contains("AND"), "should strip AND operator");
        assert!(!result.contains("OR"), "should strip OR operator");
        assert!(!result.contains("NOT"), "should strip NOT operator");
        assert!(!result.contains('"'), "should strip quotes");
        assert!(!result.contains('('), "should strip parens");
        assert!(!result.contains('*'), "should strip asterisks");
        assert!(result.contains("test"), "should preserve normal words");
        assert!(result.contains("query"), "should preserve normal words");
    }

    #[test]
    fn sanitize_fts_query_empty_input_returns_empty() {
        let result = super::sanitize_fts_query("");
        assert!(result.is_empty());
    }

    #[test]
    fn sanitize_fts_query_only_operators_returns_empty() {
        let result = super::sanitize_fts_query("AND OR NOT");
        assert!(result.is_empty());
    }

    // --- FTS integration tests ---

    // Covers: search_by_keyword returns Ok(empty) for queries with no matching documents,
    //         verifying FTS degradation does not error on non-matching input.
    #[tokio::test]
    async fn search_by_keyword_returns_empty_for_non_matching_query() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_fts", "published").await;
        insert_test_chunk(
            &store,
            "doc_fts",
            "chunk_fts_0",
            "这是一段关于内存管理的测试文本",
            "page_fts",
            Some(0),
            Some(3),
        )
        .await;

        // Manually insert FTS index for the chunk
        let store_ref = &store;
        store_ref
            .conn
            .call(move |conn| {
                // Find the rowid for the chunk we just inserted
                let rowid: i64 = conn.query_row(
                    "SELECT rowid FROM chunk_metadata WHERE chunk_id = ?",
                    rusqlite::params!["chunk_fts_0"],
                    |row| row.get(0),
                )?;

                let tokens = super::tokenize_for_fts("这是一段关于内存管理的测试文本");
                conn.execute(
                    "INSERT INTO fts_chunks(rowid, tokens) VALUES (?, ?)",
                    rusqlite::params![rowid, tokens],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("FTS insert should succeed");

        let results = store
            .search_by_keyword("xyznonexistent", 5, &RetrievalScope::Published)
            .await
            .expect("search_by_keyword should not error on non-matching query");

        assert!(
            results.is_empty(),
            "non-matching query should return empty results, got {} results",
            results.len()
        );
    }

    // --- FTS backfill tests ---

    // Covers: backfill_fts_index is idempotent — running twice does not duplicate entries.
    // Uses INSERT OR IGNORE so already-indexed rows are skipped on subsequent runs.
    // Counts via FTS MATCH query since fts_chunks is an external content table
    // and SELECT COUNT(*) triggers content-sync validation.
    #[tokio::test]
    async fn backfill_fts_index_is_idempotent() {
        let store = make_sql_only_store();

        insert_test_document(&store, "doc_bf", "published").await;
        insert_test_chunk(
            &store,
            "doc_bf",
            "chunk_bf1",
            "backfill test content about memory",
            "page_bf",
            Some(0),
            Some(2),
        )
        .await;
        insert_test_chunk(
            &store,
            "doc_bf",
            "chunk_bf2",
            "another backfill test about deployment",
            "page_bf",
            Some(1),
            Some(2),
        )
        .await;

        store
            .backfill_fts_index()
            .await
            .expect("first backfill should succeed");

        // Count by matching a term present in both chunks — "backfill"
        let count_after_first: i64 = store
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM fts_chunks WHERE fts_chunks MATCH ?",
                    rusqlite::params!["backfill"],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(
            count_after_first, 2,
            "should have 2 FTS entries after first backfill"
        );

        store
            .backfill_fts_index()
            .await
            .expect("second backfill should succeed");

        let count_after_second: i64 = store
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM fts_chunks WHERE fts_chunks MATCH ?",
                    rusqlite::params!["backfill"],
                    |row| row.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(
            count_after_second, 2,
            "should still have 2 FTS entries after second backfill (idempotent)"
        );
    }

    // --- FTS routing tests for resolve_fts_tokens ---

    // User Story: US-CORE-027
    // Covers: resolve_fts_tokens returns precomputed tokens when content_type is OpenApi
    //         and fts_tokens is Some. Must NOT call tokenize_safe in this case.
    #[test]
    fn fts_routing_uses_precomputed_tokens_for_openapi() {
        let content_type = ContentType::OpenApi;
        let fts_tokens = Some("POST api documents publish".to_string());
        let content_text = "## POST /api/documents/{documentId}/publish\n\nSome text.";

        let result = super::resolve_fts_tokens(&content_type, &fts_tokens, content_text);

        assert_eq!(
            result, "POST api documents publish",
            "should return precomputed tokens for OpenAPI content, got: {}",
            result
        );
    }

    // User Story: US-CORE-027
    // Covers: resolve_fts_tokens uses tokenize_safe when content_type is None (default),
    //         regardless of fts_tokens value.
    #[test]
    fn fts_routing_uses_tokenize_safe_for_non_openapi() {
        let content_type = ContentType::None;
        let fts_tokens = Some("ignored precomputed tokens".to_string());
        let content_text = "普通文档的文本内容";

        let result = super::resolve_fts_tokens(&content_type, &fts_tokens, content_text);

        // Should NOT return the precomputed tokens for ContentType::None
        assert_ne!(
            result, "ignored precomputed tokens",
            "should use tokenize_safe for non-OpenAPI content, not precomputed tokens"
        );
        // tokenize_safe should produce jieba-segmented output from the content
        assert!(
            !result.is_empty(),
            "tokenize_safe output should not be empty for non-empty input"
        );
    }

    // User Story: US-CORE-027
    // Covers: resolve_fts_tokens degrades gracefully to tokenize_safe when content_type
    //         is OpenApi but fts_tokens is None (missing precomputed tokens).
    #[test]
    fn fts_routing_degrades_to_tokenize_safe_when_tokens_missing() {
        let content_type = ContentType::OpenApi;
        let fts_tokens: Option<String> = None;
        let content_text = "## GET /api/health\n\nHealth check endpoint.";

        let result = super::resolve_fts_tokens(&content_type, &fts_tokens, content_text);

        // Should degrade to tokenize_safe, producing non-empty output from the content
        assert!(
            !result.is_empty(),
            "should degrade to tokenize_safe and produce non-empty output when fts_tokens is None"
        );
        // Should contain tokens from the content text (e.g. "GET", "api", "health")
        assert!(
            result.contains("health") || result.contains("GET"),
            "degraded output should contain tokens from content, got: {}",
            result
        );
    }

    // --- 写入去重（published dedup）tests ---

    /// 预置一条带真实 content_hash 和 embedding_model 的 chunk（模拟已索引状态）。
    /// insert_test_chunk 默认 content_hash/embedding_model 为 NULL，这里补齐以便
    /// find_existing_embeddings 能命中。
    async fn seed_existing_chunk(
        store: &VectorStoreManager,
        document_id: &str,
        status: &str,
        chunk_id: &str,
        content: &str,
        page_id: &str,
    ) {
        insert_test_document(store, document_id, status).await;
        insert_test_chunk(
            store,
            document_id,
            chunk_id,
            content,
            page_id,
            Some(0),
            Some(1),
        )
        .await;

        let model = store.model_name.clone();
        let hash = super::content_hash(content);
        let cid = chunk_id.to_string();
        store
            .conn
            .call(move |conn| {
                conn.execute(
                    "UPDATE chunk_metadata SET content_hash = ?, embedding_model = ? WHERE chunk_id = ?",
                    rusqlite::params![hash, model, cid],
                )?;
                Ok::<(), rusqlite::Error>(())
            })
            .await
            .expect("seed content_hash should succeed");
    }

    /// 构造一个最小 DocumentChunk（content 为单元素 Vec，与 index 的 hash 计算口径一致）。
    fn make_chunk(document_id: &str, page_id: &str, content: &str) -> DocumentChunk {
        DocumentChunk {
            id: format!("{}:0", page_id),
            document_id: document_id.to_string(),
            page_id: page_id.to_string(),
            content: vec![content.to_string()],
            ..Default::default()
        }
    }

    async fn count_chunks_for_doc(store: &VectorStoreManager, document_id: &str) -> i64 {
        let doc_id = document_id.to_string();
        store
            .conn
            .call(move |conn| {
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM chunk_metadata WHERE document_id = ?",
                    rusqlite::params![doc_id],
                    |row| row.get(0),
                )?;
                Ok::<i64, rusqlite::Error>(count)
            })
            .await
            .expect("count should succeed")
    }

    // Covers: 重复内容已存在于 published 文档 → 新文档仍写入自包含条目（复用向量）。
    //         验证新文档独立创建 chunk_metadata，reused 计数正确，无重复向量化。
    #[tokio::test]
    async fn index_reuses_vector_when_content_matches_published() {
        let store = make_sql_only_store();

        seed_existing_chunk(
            &store,
            "doc_published",
            "published",
            "chunk_pub",
            "duplicate content",
            "page_pub",
        )
        .await;

        let new_doc = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2c01").unwrap();
        let chunk = make_chunk(&new_doc.to_string(), "page_new", "duplicate content");

        let stats = store
            .index_document_with_options(new_doc, vec![chunk], IndexOptions::default())
            .await
            .expect("index should succeed");

        assert_eq!(stats.reused, 1, "应复用已缓存的向量");
        assert_eq!(stats.indexed, 1, "应写入 1 条 chunk（自包含）");

        let count = count_chunks_for_doc(&store, &new_doc.to_string()).await;
        assert_eq!(count, 1, "新文档应写入自包含 chunk 条目");
    }

    // Covers: 重复内容仅存在于 draft 文档 → 新文档写入自包含条目（复用向量）。
    #[tokio::test]
    async fn index_reuses_vector_when_content_matches_draft() {
        let store = make_sql_only_store();

        seed_existing_chunk(
            &store,
            "doc_draft",
            "draft",
            "chunk_draft",
            "draft only content",
            "page_draft",
        )
        .await;

        let new_doc = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2c02").unwrap();
        let chunk = make_chunk(&new_doc.to_string(), "page_new", "draft only content");

        let stats = store
            .index_document_with_options(new_doc, vec![chunk], IndexOptions::default())
            .await
            .expect("index should succeed");

        assert_eq!(stats.reused, 1, "应复用已缓存的向量");
        assert_eq!(stats.indexed, 1, "应写入 1 条 chunk（自包含）");

        let count = count_chunks_for_doc(&store, &new_doc.to_string()).await;
        assert_eq!(count, 1, "新文档应写入 1 条 chunk");
    }

    // Covers: refresh_embed=true 时强制重新向量化，不复用缓存（但仍写入自包含条目）。
    // 用 mockito 模拟 OpenAI /embeddings 端点，使强制重算路径可离线执行。
    // 关键点：若 refresh_embed 未生效（误复用缓存），则不会发起 /embeddings 请求，
    // mock.assert 会失败——把“是否真的重新向量化”纳入断言，而不只是看计数。
    #[tokio::test]
    async fn refresh_embed_forces_reembedding() {
        let mut server = mockito::Server::new_async().await;

        // 1536 维零向量，与 text-embedding-3-small / vec_chunks(float[1536]) 维度一致。
        let embedding: Vec<f64> = vec![0.0; 1536];
        let mock = server
            .mock("POST", "/embeddings")
            .with_status(200)
            .with_body(
                serde_json::json!({
                    "object": "list",
                    "data": [{ "object": "embedding", "index": 0, "embedding": embedding }],
                    "model": "text-embedding-3-small",
                    "usage": { "prompt_tokens": 1, "total_tokens": 1 }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let store = make_store_with_embedding_base_url(&server.url());

        seed_existing_chunk(
            &store,
            "doc_pub",
            "published",
            "chunk_pub",
            "refresh dedup content",
            "page_pub",
        )
        .await;

        let new_doc = Uuid::parse_str("01918170-7c21-7d2e-8e64-7e6f6c1a2c03").unwrap();
        let chunk = make_chunk(&new_doc.to_string(), "page_new", "refresh dedup content");

        let stats = store
            .index_document_with_options(
                new_doc,
                vec![chunk],
                IndexOptions {
                    refresh_embed: true,
                },
            )
            .await
            .expect("index should succeed");

        assert_eq!(stats.reused, 0, "refresh_embed=true 时不复用缓存向量");
        assert_eq!(stats.indexed, 1, "应写入 1 条 chunk（强制重新向量化）");

        let count = count_chunks_for_doc(&store, &new_doc.to_string()).await;
        assert_eq!(count, 1, "新文档应写入自包含 chunk 条目");

        // 强制重算必须真的命中 embedding 端点；若误走复用路径则此处失败。
        mock.assert_async().await;
    }
}
