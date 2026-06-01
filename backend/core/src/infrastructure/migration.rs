use rusqlite_migration::{Migrations, M};

/// Build database migrations. `ndims` is the embedding model's output dimensionality,
/// used to define the vector column width in vec_chunks.
pub fn migrations(ndims: usize) -> Migrations<'static> {
    // Leaked to satisfy M::up's &'static str requirement; called once per process start.
    let vec_sql: &str = Box::leak(
        format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_chunks USING vec0(embedding float[{}]);",
            ndims
        )
        .into_boxed_str(),
    );
    Migrations::new(vec![
        M::up(
            "CREATE TABLE IF NOT EXISTS documents (\
                id TEXT PRIMARY KEY,\
                file_name TEXT NOT NULL,\
                status TEXT NOT NULL DEFAULT 'processing',\
                row_count INTEGER DEFAULT 0,\
                error_message TEXT,\
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))\
            );\
            CREATE TABLE IF NOT EXISTS chunk_metadata (\
                rowid INTEGER PRIMARY KEY,\
                document_id TEXT NOT NULL,\
                chunk_id TEXT NOT NULL,\
                content TEXT NOT NULL,\
                title TEXT NOT NULL DEFAULT '',\
                locale TEXT,\
                link TEXT,\
                tags TEXT NOT NULL DEFAULT '',\
                section TEXT,\
                content_hash TEXT,\
                embedding_model TEXT,\
                page_id TEXT NOT NULL DEFAULT '',\
                sub_index INTEGER DEFAULT NULL,\
                chunk_count INTEGER DEFAULT NULL\
            );\
            CREATE INDEX IF NOT EXISTS idx_chunk_metadata_document_id ON chunk_metadata(document_id);\
            CREATE INDEX IF NOT EXISTS idx_chunk_metadata_content_hash ON chunk_metadata(content_hash);\
            CREATE INDEX IF NOT EXISTS idx_chunk_metadata_page_sub ON chunk_metadata(page_id, sub_index);",
        ),
        M::up(vec_sql),
    ])
}
