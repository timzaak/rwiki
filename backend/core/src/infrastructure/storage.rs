use std::collections::HashMap;
use std::future::Future;

use uuid::Uuid;

use crate::domain::errors::CoreError;

use super::vector_store::{NeighborChunk, RetrievalScope, SearchResult};

/// 检索与索引存储边界 —— 原 VectorStoreManager 的全部 SQL 面。
/// 语义契约：与 SQLite 现状逐方法等价；两实现（SqliteStore/PgStore）签名由本 trait 锚定。
pub trait RetrievalStore: Send + Sync {
    /// 索引写入：单事务写入一个文档的全部 chunk
    /// （chunk_metadata 行 + 向量 + 预分词文本/fts）。rows 为空时为无操作。
    fn insert_chunks(
        &self,
        document_id: &str,
        rows: Vec<NewChunkRow>,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 按 content_hash 查缓存向量（仅同 embedding_model 的行），
    /// 供索引去重复用 embedding。返回 hash → f32 向量。
    fn find_cached_embeddings(
        &self,
        content_hashes: &[String],
        model_name: &str,
    ) -> impl Future<Output = Result<HashMap<String, Vec<f32>>, CoreError>> + Send;

    /// 分批补齐 content_hash/embedding_model（SQLite 旧数据专用；PG 新库恒为 0 行，
    /// 实现为立即返回 Ok(0)，保持启动链单一路径）。
    fn backfill_content_hash(
        &self,
        model_name: &str,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// 分批补齐 fts_chunks FTS5 索引（SQLite 旧数据专用，幂等 INSERT OR IGNORE；
    /// PG 侧 fts 为生成列随 chunk 行写入自动派生，实现恒为 Ok(())。
    /// 与 backfill_content_hash 同类：启动期补齐统一走 trait，保持启动链单一路径）。
    fn backfill_fts_index(&self) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 向量 KNN 检索。返回的 SearchResult.score 已归一到 SQLite L2 标度（cosine_distance_to_score）。
    /// scope 语义（发布过滤/频道/集合）由实现渲染为各自 SQL 谓词。
    fn search_by_vector(
        &self,
        query_vec: &[f64],
        top_k: usize,
        scope: &RetrievalScope,
    ) -> impl Future<Output = Result<Vec<SearchResult>, CoreError>> + Send;

    /// 关键词检索。入参 fts_query 为应用层已完成
    /// tokenize_for_fts + sanitize_fts_query 的空格连接 token 串
    /// （分词单源；实现不得再分词）。索引缺失/查询失败按现状降级为空结果。
    fn search_by_keyword(
        &self,
        fts_query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> impl Future<Output = Result<Vec<SearchResult>, CoreError>> + Send;

    /// 邻居 chunk 扩展（page_id + sub_index 区间，chunk_count 非空的老数据规则不变）。
    fn get_neighbor_chunks(
        &self,
        page_id: &str,
        start_sub: i64,
        end_sub: i64,
        scope: &RetrievalScope,
    ) -> impl Future<Output = Result<Vec<NeighborChunk>, CoreError>> + Send;

    /// 知识库是否为空（count(chunk_metadata) == 0）。
    fn is_empty(&self) -> impl Future<Output = Result<bool, CoreError>> + Send;

    /// 删除文档全部 chunk 及向量/索引（幂等：无 chunk 时 Ok）。
    fn remove_document_chunks(
        &self,
        document_id: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;
}

/// 元数据 CRUD 边界 —— 原 handlers 直连 SQL + 启动/健康/指标三处旁路面。
pub trait MetadataStore: Send + Sync {
    /// 存储连通性探测（SQLite: SELECT 1；PG: 池取连接执行 SELECT 1）。
    fn ping(&self) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 上传登记：INSERT documents (status='processing')。
    fn insert_document(
        &self,
        doc: NewDocument,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 按 id（可选限定 channel）取整行；None = 不存在。
    /// 覆盖现状：失败回查（按 id）、发布/取消发布回查与存在性检查（按 id+channel）。
    fn get_document(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> impl Future<Output = Result<Option<DocumentRecord>, CoreError>> + Send;

    /// 按频道列文档（created_at 倒序）。
    fn list_documents_by_channel(
        &self,
        channel_id: &str,
    ) -> impl Future<Output = Result<Vec<DocumentRecord>, CoreError>> + Send;

    /// 守卫状态更新（publish: from='draft'→'published'；unpublish: 反向；
    /// 上传成功路径 from=None→'draft'，仅按 id）。返回受影响行数（0 = 守卫未命中）。
    fn set_document_status(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
        from_status: Option<&str>,
        to_status: &str,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// 上传失败标记（status='failed' + error_message，按 id）。
    fn mark_document_failed(
        &self,
        document_id: &str,
        error_message: &str,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 批量读取指定频道内 id → status（PG 单语句，SQLite 保留 900 分块语义，
    /// 对调用方透明——统一为"返回全部命中行"）。
    fn document_statuses(
        &self,
        channel_id: &str,
        ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<(String, String)>, CoreError>> + Send;

    /// 批量守卫状态更新（publish 要求 status='draft'，unpublish 要求 'published'），
    /// 单事务；返回 (id, applied) 列表，供 handler 校正响应中的 applied 标志。
    fn set_documents_status(
        &self,
        channel_id: &str,
        publish_ids: &[Uuid],
        unpublish_ids: &[Uuid],
    ) -> impl Future<Output = Result<Vec<(Uuid, bool)>, CoreError>> + Send;

    /// 删除 documents 行（channel=None 为按 id；delete 接口按 id+channel）。幂等。
    fn delete_document_row(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// 频道是否有已发布文档（单/多频道，任一命中即 true；失败按现状返回 false）。
    fn has_published_documents_for_channel(
        &self,
        channel_id: &str,
    ) -> impl Future<Output = bool> + Send;
    fn has_published_documents_for_channels(
        &self,
        channel_ids: &[String],
    ) -> impl Future<Output = bool> + Send;

    /// 指标：按状态计数文档 / chunk 总数（ObservableGauge 数据源）。
    fn count_documents_with_status(
        &self,
        status: &str,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;
    fn count_chunks(&self) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// 启动恢复：processing → failed。返回受影响行数。
    fn recover_stuck_processing_documents(
        &self,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;

    /// 启动期历史数据校验：channel_id 为空的行数必须为 0。SQLite 轨道校验
    /// documents / chat_feedback / low_recall_records 三表；PG 轨道三表
    /// channel_id NOT NULL，该类行不可能存在，实现恒为 Ok(())。
    fn validate_historical_rows_have_channel_id(
        &self,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 反馈 upsert：feedback=None → 删除自然键行（幂等）；Some → 事务内
    /// DELETE 自然键行 + INSERT（精确复刻 SQLite INSERT OR REPLACE，含重新分配 id）。
    fn upsert_feedback(
        &self,
        feedback: NewFeedback,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 反馈列表（频道 + 可选 like/dislike 过滤 + 分页；返回条目与 total）。
    fn list_feedback(
        &self,
        channel_id: &str,
        feedback: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<FeedbackPage, CoreError>> + Send;

    /// 低相关记录写入（chat spawned 任务旁路，失败仅告警）。
    fn insert_low_recall_record(
        &self,
        record: NewLowRecallRecord,
    ) -> impl Future<Output = Result<(), CoreError>> + Send;

    /// 低相关记录列表（动态过滤：channel 必选，min/max_score、from/to_ts 可选 + 分页）。
    fn list_low_recall_records(
        &self,
        filter: LowRecallFilter,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<LowRecallPage, CoreError>> + Send;
}

/// 进程内存储访问单通道：AppState 与 VectorStoreManager 共同持有。
/// 由启动链按配置构造（storage_postgres::startup / main.rs SQLite 分支）；Clone 廉价。
#[derive(Clone)]
pub enum Storage {
    Sqlite(super::storage_sqlite::SqliteStore),
    Postgres(super::storage_postgres::PgStore),
}

impl RetrievalStore for Storage {
    async fn insert_chunks(
        &self,
        document_id: &str,
        rows: Vec<NewChunkRow>,
    ) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.insert_chunks(document_id, rows).await,
            Storage::Postgres(s) => s.insert_chunks(document_id, rows).await,
        }
    }

    async fn find_cached_embeddings(
        &self,
        content_hashes: &[String],
        model_name: &str,
    ) -> Result<HashMap<String, Vec<f32>>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.find_cached_embeddings(content_hashes, model_name).await,
            Storage::Postgres(s) => s.find_cached_embeddings(content_hashes, model_name).await,
        }
    }

    async fn backfill_content_hash(&self, model_name: &str) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => s.backfill_content_hash(model_name).await,
            Storage::Postgres(s) => s.backfill_content_hash(model_name).await,
        }
    }

    async fn backfill_fts_index(&self) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.backfill_fts_index().await,
            Storage::Postgres(s) => s.backfill_fts_index().await,
        }
    }

    async fn search_by_vector(
        &self,
        query_vec: &[f64],
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.search_by_vector(query_vec, top_k, scope).await,
            Storage::Postgres(s) => s.search_by_vector(query_vec, top_k, scope).await,
        }
    }

    async fn search_by_keyword(
        &self,
        fts_query: &str,
        top_k: usize,
        scope: &RetrievalScope,
    ) -> Result<Vec<SearchResult>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.search_by_keyword(fts_query, top_k, scope).await,
            Storage::Postgres(s) => s.search_by_keyword(fts_query, top_k, scope).await,
        }
    }

    async fn get_neighbor_chunks(
        &self,
        page_id: &str,
        start_sub: i64,
        end_sub: i64,
        scope: &RetrievalScope,
    ) -> Result<Vec<NeighborChunk>, CoreError> {
        match self {
            Storage::Sqlite(s) => {
                s.get_neighbor_chunks(page_id, start_sub, end_sub, scope)
                    .await
            }
            Storage::Postgres(s) => {
                s.get_neighbor_chunks(page_id, start_sub, end_sub, scope)
                    .await
            }
        }
    }

    async fn is_empty(&self) -> Result<bool, CoreError> {
        match self {
            Storage::Sqlite(s) => s.is_empty().await,
            Storage::Postgres(s) => s.is_empty().await,
        }
    }

    async fn remove_document_chunks(&self, document_id: &str) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.remove_document_chunks(document_id).await,
            Storage::Postgres(s) => s.remove_document_chunks(document_id).await,
        }
    }
}

impl MetadataStore for Storage {
    async fn ping(&self) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.ping().await,
            Storage::Postgres(s) => s.ping().await,
        }
    }

    async fn insert_document(&self, doc: NewDocument) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.insert_document(doc).await,
            Storage::Postgres(s) => s.insert_document(doc).await,
        }
    }

    async fn get_document(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<Option<DocumentRecord>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.get_document(document_id, channel_id).await,
            Storage::Postgres(s) => s.get_document(document_id, channel_id).await,
        }
    }

    async fn list_documents_by_channel(
        &self,
        channel_id: &str,
    ) -> Result<Vec<DocumentRecord>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.list_documents_by_channel(channel_id).await,
            Storage::Postgres(s) => s.list_documents_by_channel(channel_id).await,
        }
    }

    async fn set_document_status(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
        from_status: Option<&str>,
        to_status: &str,
    ) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => {
                s.set_document_status(document_id, channel_id, from_status, to_status)
                    .await
            }
            Storage::Postgres(s) => {
                s.set_document_status(document_id, channel_id, from_status, to_status)
                    .await
            }
        }
    }

    async fn mark_document_failed(
        &self,
        document_id: &str,
        error_message: &str,
    ) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.mark_document_failed(document_id, error_message).await,
            Storage::Postgres(s) => s.mark_document_failed(document_id, error_message).await,
        }
    }

    async fn document_statuses(
        &self,
        channel_id: &str,
        ids: &[Uuid],
    ) -> Result<Vec<(String, String)>, CoreError> {
        match self {
            Storage::Sqlite(s) => s.document_statuses(channel_id, ids).await,
            Storage::Postgres(s) => s.document_statuses(channel_id, ids).await,
        }
    }

    async fn set_documents_status(
        &self,
        channel_id: &str,
        publish_ids: &[Uuid],
        unpublish_ids: &[Uuid],
    ) -> Result<Vec<(Uuid, bool)>, CoreError> {
        match self {
            Storage::Sqlite(s) => {
                s.set_documents_status(channel_id, publish_ids, unpublish_ids)
                    .await
            }
            Storage::Postgres(s) => {
                s.set_documents_status(channel_id, publish_ids, unpublish_ids)
                    .await
            }
        }
    }

    async fn delete_document_row(
        &self,
        document_id: &str,
        channel_id: Option<&str>,
    ) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => s.delete_document_row(document_id, channel_id).await,
            Storage::Postgres(s) => s.delete_document_row(document_id, channel_id).await,
        }
    }

    async fn has_published_documents_for_channel(&self, channel_id: &str) -> bool {
        match self {
            Storage::Sqlite(s) => s.has_published_documents_for_channel(channel_id).await,
            Storage::Postgres(s) => s.has_published_documents_for_channel(channel_id).await,
        }
    }

    async fn has_published_documents_for_channels(&self, channel_ids: &[String]) -> bool {
        match self {
            Storage::Sqlite(s) => s.has_published_documents_for_channels(channel_ids).await,
            Storage::Postgres(s) => s.has_published_documents_for_channels(channel_ids).await,
        }
    }

    async fn count_documents_with_status(&self, status: &str) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => s.count_documents_with_status(status).await,
            Storage::Postgres(s) => s.count_documents_with_status(status).await,
        }
    }

    async fn count_chunks(&self) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => s.count_chunks().await,
            Storage::Postgres(s) => s.count_chunks().await,
        }
    }

    async fn recover_stuck_processing_documents(&self) -> Result<u64, CoreError> {
        match self {
            Storage::Sqlite(s) => s.recover_stuck_processing_documents().await,
            Storage::Postgres(s) => s.recover_stuck_processing_documents().await,
        }
    }

    async fn validate_historical_rows_have_channel_id(&self) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.validate_historical_rows_have_channel_id().await,
            Storage::Postgres(s) => s.validate_historical_rows_have_channel_id().await,
        }
    }

    async fn upsert_feedback(&self, feedback: NewFeedback) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.upsert_feedback(feedback).await,
            Storage::Postgres(s) => s.upsert_feedback(feedback).await,
        }
    }

    async fn list_feedback(
        &self,
        channel_id: &str,
        feedback: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<FeedbackPage, CoreError> {
        match self {
            Storage::Sqlite(s) => s.list_feedback(channel_id, feedback, limit, offset).await,
            Storage::Postgres(s) => s.list_feedback(channel_id, feedback, limit, offset).await,
        }
    }

    async fn insert_low_recall_record(&self, record: NewLowRecallRecord) -> Result<(), CoreError> {
        match self {
            Storage::Sqlite(s) => s.insert_low_recall_record(record).await,
            Storage::Postgres(s) => s.insert_low_recall_record(record).await,
        }
    }

    async fn list_low_recall_records(
        &self,
        filter: LowRecallFilter,
        limit: i64,
        offset: i64,
    ) -> Result<LowRecallPage, CoreError> {
        match self {
            Storage::Sqlite(s) => s.list_low_recall_records(filter, limit, offset).await,
            Storage::Postgres(s) => s.list_low_recall_records(filter, limit, offset).await,
        }
    }
}

/// 索引写入行（VectorStoreManager 完成去重/embedding/分词后组装）。
pub struct NewChunkRow {
    pub chunk_id: String,
    pub content: String,
    pub title: String,
    pub locale: Option<String>,
    pub link: Option<String>,
    /// 实现侧自行序列化（SQLite `\x00` / PG `\x1f`）。
    pub tags: Vec<String>,
    pub section: Option<String>,
    pub page_id: String,
    pub sub_index: Option<i64>,
    pub chunk_count: Option<i64>,
    pub content_hash: String,
    /// 写入 chunk_metadata.embedding_model 的模型名（缓存有效性判断键）。
    pub embedding_model: String,
    /// 已完成 f64→f32 换算（沿用现状精度口径）。
    pub embedding: Vec<f32>,
    /// resolve_fts_tokens 输出（应用层预分词，空格连接 token 串）。
    pub fts_tokens: String,
}

pub struct NewDocument {
    /// UUIDv7 字符串
    pub id: String,
    pub file_name: String,
    pub row_count: i32,
    pub channel_id: String,
}

/// documents 行（created_at 统一 ISO-8601 UTC 字符串，格式与 SQLite 现状一致：
/// "%Y-%m-%dT%H:%M:%S%.3fZ"，由 PG 实现读取时格式化）。
#[derive(Debug, Clone)]
pub struct DocumentRecord {
    pub id: String,
    pub file_name: String,
    pub status: String,
    pub row_count: i32,
    pub channel_id: String,
    pub error_message: Option<String>,
    pub created_at: String,
}

pub struct NewFeedback {
    pub channel_id: String,
    pub session_id: String,
    pub message_id: String,
    /// None = 清除反馈
    pub feedback: Option<String>,
    pub user_message: String,
    pub assistant_message: String,
}

/// chat_feedback 行（created_at 为 ISO 字符串）。
#[derive(Debug, Clone)]
pub struct FeedbackRecord {
    pub id: i64,
    pub channel_id: String,
    pub session_id: String,
    pub message_id: String,
    pub feedback: String,
    pub user_message: String,
    pub assistant_message: String,
    pub created_at: String,
}

pub struct FeedbackPage {
    pub items: Vec<FeedbackRecord>,
    pub total: i64,
}

pub struct NewLowRecallRecord {
    pub session_id: Option<String>,
    pub channel_id: String,
    pub query: String,
    pub top_score: Option<f64>,
    pub result_count: i64,
    /// 来源 JSON（应用层序列化，可移植）
    pub sources_json: String,
}

pub struct LowRecallFilter {
    pub channel_id: String,
    pub min_score: Option<f64>,
    pub max_score: Option<f64>,
    /// ISO-8601 字符串（与 created_at 存储格式同口径可比）
    pub from_ts: Option<String>,
    pub to_ts: Option<String>,
}

/// low_recall_records 行（sources 为原始 JSON 字符串，解析归 api 层 DTO）。
#[derive(Debug, Clone)]
pub struct LowRecallRecordRow {
    pub id: i64,
    pub channel_id: String,
    pub session_id: Option<String>,
    pub query: String,
    pub top_score: Option<f64>,
    pub result_count: i64,
    pub sources_json: String,
    pub created_at: String,
}

pub struct LowRecallPage {
    pub items: Vec<LowRecallRecordRow>,
    pub total: i64,
}

/// set_documents_status 的守卫分类（两后端共享的纯逻辑）：
/// publish 要求 status='draft'，unpublish 要求 status='published'。
/// 返回 (applied_flags, 命中 publish 守卫的 ids, 命中 unpublish 守卫的 ids)。
pub(crate) fn classify_batch_status_guards(
    publish_ids: &[Uuid],
    unpublish_ids: &[Uuid],
    current_statuses: &HashMap<Uuid, String>,
) -> (Vec<(Uuid, bool)>, Vec<Uuid>, Vec<Uuid>) {
    let mut applied_flags = Vec::with_capacity(publish_ids.len() + unpublish_ids.len());
    let mut valid_publish_ids = Vec::new();
    let mut valid_unpublish_ids = Vec::new();

    for id in publish_ids {
        let applied = matches!(current_statuses.get(id), Some(status) if status == "draft");
        if applied {
            valid_publish_ids.push(*id);
        }
        applied_flags.push((*id, applied));
    }
    for id in unpublish_ids {
        let applied = matches!(current_statuses.get(id), Some(status) if status == "published");
        if applied {
            valid_unpublish_ids.push(*id);
        }
        applied_flags.push((*id, applied));
    }

    (applied_flags, valid_publish_ids, valid_unpublish_ids)
}

/// PG cosine 距离 → SQLite vec0 L2 分数标度。
/// 现状：vec0 默认 L2，score = 1 - L2。
/// 单位向量下 L2 = sqrt(2 - 2·cos_sim) = sqrt(2·cos_distance)，
/// 故映射 score = 1 - sqrt(2·cos_distance) 在单位向量前提下与 SQLite 数值一致。
/// 本项目全部 embedding provider 默认返回单位向量；
/// 非单位向量时排序仍单调等价（L2 与 cosine 距离对固定模长向量单调对应）。
pub(crate) fn cosine_distance_to_score(cosine_distance: f32) -> f64 {
    1.0 - (2.0 * f64::from(cosine_distance)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Covers: 分数标度归一 —— PG cosine 距离 0（同向）与 SQLite 1 - L2 的 1.0 对齐。
    #[test]
    fn cosine_distance_to_score_identity_distance_is_one() {
        let score = cosine_distance_to_score(0.0);
        assert!(
            (score - 1.0).abs() < 1e-9,
            "cos_distance=0 → score=1, got {score}"
        );
    }

    // Covers: 分数标度归一 —— 正交单位向量 cos_distance=0.5 → L2=sqrt(2·0.5)=1 → score=0。
    #[test]
    fn cosine_distance_to_score_orthogonal_distance_is_zero() {
        let score = cosine_distance_to_score(0.5);
        assert!(
            (score - 0.0).abs() < 1e-9,
            "cos_distance=0.5 → score=0, got {score}"
        );
    }

    // Covers: 分数标度归一 —— 单位向量下本映射与 1 - L2 逐点一致（数值相等而非仅排序相等），
    // 这是低相关阈值跨后端同义、eval 分数可直接对比的前提。
    #[test]
    fn cosine_distance_to_score_matches_sqlite_l2_scale_on_unit_vectors() {
        for cos_sim_f32 in [1.0f32, 0.9, 0.5, 0.0, -0.5] {
            let cos_sim = f64::from(cos_sim_f32);
            let a = [1.0f64, 0.0, 0.0];
            let b = [cos_sim, (1.0 - cos_sim * cos_sim).sqrt(), 0.0];
            let l2_sq: f64 = a.iter().zip(&b).map(|(x, y)| (x - y).powi(2)).sum();
            let sqlite_score = 1.0 - l2_sq.sqrt();

            let cos_dist = 1.0 - cos_sim;
            let pg_score = cosine_distance_to_score(cos_dist as f32);

            assert!(
                (sqlite_score - pg_score).abs() < 1e-3,
                "unit-vector scores should match numerically: sqlite={sqlite_score}, pg={pg_score} (cos_sim={cos_sim})"
            );
        }
    }
}
