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
        M::up(
            "CREATE VIRTUAL TABLE IF NOT EXISTS fts_chunks USING fts5(\
                tokens,\
                content='chunk_metadata',\
                content_rowid='rowid',\
                tokenize=\"unicode61\"\
            );",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS chat_feedback (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                session_id TEXT NOT NULL,\
                message_id TEXT NOT NULL,\
                feedback TEXT NOT NULL CHECK(feedback IN ('like', 'dislike')),\
                user_message TEXT NOT NULL,\
                assistant_message TEXT NOT NULL,\
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),\
                UNIQUE(session_id, message_id)\
            );\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_created_at ON chat_feedback(created_at DESC);\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_feedback ON chat_feedback(feedback);",
        ),
        M::up(
            "CREATE TABLE IF NOT EXISTS low_recall_records (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                session_id TEXT,\
                query TEXT NOT NULL,\
                top_score REAL,\
                result_count INTEGER NOT NULL DEFAULT 0,\
                sources TEXT NOT NULL DEFAULT '[]',\
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))\
            );\
            CREATE INDEX IF NOT EXISTS idx_low_recall_created_at ON low_recall_records(created_at DESC);\
            CREATE INDEX IF NOT EXISTS idx_low_recall_top_score ON low_recall_records(top_score);",
        ),
        M::up(
            "ALTER TABLE documents ADD COLUMN site_id TEXT;\
            CREATE INDEX IF NOT EXISTS idx_documents_site_id ON documents(site_id);\
            ALTER TABLE chat_feedback ADD COLUMN site_id TEXT;\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_site_id ON chat_feedback(site_id);\
            ALTER TABLE low_recall_records ADD COLUMN site_id TEXT;\
            CREATE INDEX IF NOT EXISTS idx_low_recall_site_id ON low_recall_records(site_id);",
        ),
        // Recreate chat_feedback so its natural key includes site_id, preventing
        // identical session/message ids from different sites from overwriting each
        // other. site_id remains nullable here so the migration can complete when
        // historical rows have not yet been backfilled; application code enforces
        // non-empty values and startup validation rejects NULL site_id.
        M::up(
            "CREATE TABLE chat_feedback_new (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                site_id TEXT,\
                session_id TEXT NOT NULL,\
                message_id TEXT NOT NULL,\
                feedback TEXT NOT NULL CHECK(feedback IN ('like', 'dislike')),\
                user_message TEXT NOT NULL,\
                assistant_message TEXT NOT NULL,\
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),\
                UNIQUE(site_id, session_id, message_id)\
            );\
            INSERT INTO chat_feedback_new (id, site_id, session_id, message_id, feedback, user_message, assistant_message, created_at)\
                SELECT id, site_id, session_id, message_id, feedback, user_message, assistant_message, created_at FROM chat_feedback;\
            DROP TABLE chat_feedback;\
            ALTER TABLE chat_feedback_new RENAME TO chat_feedback;\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_created_at ON chat_feedback(created_at DESC);\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_feedback ON chat_feedback(feedback);\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_site_id ON chat_feedback(site_id);",
        ),
        // Rename site_id columns to channel_id in all scoped tables, recreate the
        // chat_feedback unique key around channel_id, and rebuild named indexes.
        M::up(
            "ALTER TABLE documents RENAME COLUMN site_id TO channel_id;\
            DROP INDEX IF EXISTS idx_documents_site_id;\
            CREATE INDEX IF NOT EXISTS idx_documents_channel_id ON documents(channel_id);\
            ALTER TABLE low_recall_records RENAME COLUMN site_id TO channel_id;\
            DROP INDEX IF EXISTS idx_low_recall_site_id;\
            CREATE INDEX IF NOT EXISTS idx_low_recall_channel_id ON low_recall_records(channel_id);\
            CREATE TABLE chat_feedback_new (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                channel_id TEXT,\
                session_id TEXT NOT NULL,\
                message_id TEXT NOT NULL,\
                feedback TEXT NOT NULL CHECK(feedback IN ('like', 'dislike')),\
                user_message TEXT NOT NULL,\
                assistant_message TEXT NOT NULL,\
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),\
                UNIQUE(channel_id, session_id, message_id)\
            );\
            INSERT INTO chat_feedback_new (id, channel_id, session_id, message_id, feedback, user_message, assistant_message, created_at)\
                SELECT id, site_id, session_id, message_id, feedback, user_message, assistant_message, created_at FROM chat_feedback;\
            DROP TABLE chat_feedback;\
            ALTER TABLE chat_feedback_new RENAME TO chat_feedback;\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_created_at ON chat_feedback(created_at DESC);\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_feedback ON chat_feedback(feedback);\
            CREATE INDEX IF NOT EXISTS idx_chat_feedback_channel_id ON chat_feedback(channel_id);",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn make_conn() -> rusqlite::Connection {
        ensure_sqlite_vec_loaded();
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .expect("set wal mode");
        conn
    }

    // Covers: BE-D04 chat_feedback migration — the natural key must include channel_id
    // so identical session/message ids from different channels cannot overwrite each other.
    // Historical rows with NULL channel_id must migrate without failure (startup validation
    // enforces backfill before the service actually runs).
    #[test]
    fn chat_feedback_migration_includes_channel_id_in_unique_key() {
        let mut conn = make_conn();

        // Apply migrations up to and including the one that adds site_id (version 6).
        migrations(1536)
            .to_version(&mut conn, 6)
            .expect("apply migrations through site_id column addition");

        // Seed a pre-BE-D04 feedback row with NULL site_id.
        conn.execute(
            "INSERT INTO chat_feedback (session_id, message_id, feedback, user_message, assistant_message)
             VALUES ('s1', 'm1', 'like', 'hello', 'hi')",
            [],
        )
        .expect("insert historical feedback row");

        // Apply the rename/recreation migration.
        migrations(1536)
            .to_latest(&mut conn)
            .expect("apply chat_feedback rename/recreation migration");

        // channel_id column must exist.
        let channel_id_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('chat_feedback') WHERE name = 'channel_id'",
                [],
                |row| row.get(0),
            )
            .expect("query table info");
        assert_eq!(
            channel_id_count, 1,
            "chat_feedback must have channel_id column"
        );

        // A unique index originating from the table schema must include channel_id.
        let unique_with_channel_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_index_info(
                    (SELECT name FROM pragma_index_list('chat_feedback') WHERE origin = 'u' LIMIT 1)
                ) WHERE name = 'channel_id'",
                [],
                |row| row.get(0),
            )
            .expect("query unique index info");
        assert_eq!(
            unique_with_channel_count, 1,
            "chat_feedback unique key must include channel_id"
        );

        // Historical row is preserved.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_feedback WHERE session_id = 's1' AND message_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .expect("count historical row");
        assert_eq!(count, 1, "historical feedback row should be preserved");

        // New rows with the same session/message but different channel_id do not conflict.
        conn.execute(
            "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message)
             VALUES ('channel_a', 's1', 'm1', 'dislike', 'hello', 'hi')",
            [],
        )
        .expect("insert channel-scoped feedback row");
        conn.execute(
            "INSERT INTO chat_feedback (channel_id, session_id, message_id, feedback, user_message, assistant_message)
             VALUES ('channel_b', 's1', 'm1', 'like', 'hello', 'hi')",
            [],
        )
        .expect("insert second channel-scoped feedback row");
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_feedback WHERE session_id = 's1' AND message_id = 'm1'",
                [],
                |row| row.get(0),
            )
            .expect("count all rows");
        assert_eq!(total, 3, "channel_id must scope the natural key");
    }
}
