pub mod document_chunk;
pub mod embedding_model;
pub mod faq_parser;
pub mod markdown_parser;
pub mod metrics;
pub mod migration;
pub mod openapi_parser;
pub mod openapi_tokenizer;
pub mod otel;
pub mod pg_migration;
pub mod reranker;
pub mod rig_client;
pub mod storage;
pub mod storage_postgres;
pub mod storage_sqlite;
pub mod text_chunker;
pub mod vector_store;
pub mod xlsx_parser;

#[cfg(any(test, feature = "test-support"))]
pub mod storage_testing;

#[cfg(test)]
mod faq_parser_scenarios;
#[cfg(test)]
mod markdown_parser_scenarios;
#[cfg(test)]
mod metrics_scenarios;
#[cfg(test)]
mod openapi_parser_scenarios;
#[cfg(test)]
mod reranker_scenarios;
#[cfg(test)]
mod storage_scenarios;
#[cfg(test)]
mod text_chunker_scenarios;
#[cfg(test)]
mod vector_store_scenarios;
#[cfg(test)]
mod xlsx_parser_scenarios;
