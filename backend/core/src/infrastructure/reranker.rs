use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Rerank 请求结果
#[derive(Debug, Clone, PartialEq)]
pub struct RerankResult {
    /// 原始结果在输入 documents 中的索引
    pub index: usize,
    /// 相关性分数（归一化，跨查询不可比较）
    pub relevance_score: f64,
}

/// Rerank 错误类型
#[derive(Debug, thiserror::Error)]
pub enum RerankError {
    /// 网络请求错误（连接失败、DNS 解析失败等）
    #[error("rerank network error: {0}")]
    Network(String),
    /// API 返回非 2xx 状态码
    #[error("rerank API error (status {status}): {message}")]
    Api { status: u16, message: String },
    /// 请求超过配置的超时时间
    #[error("rerank request timed out after {0:?}")]
    Timeout(Duration),
    /// 响应体无法解析为预期 JSON 结构
    #[error("rerank response parse error: {0}")]
    ResponseParse(String),
}

/// Reranker 统一接口
pub trait Reranker: Send + Sync {
    /// 对候选文档进行 rerank 精排
    /// - query: 用户查询
    /// - documents: 候选文档内容列表（为空时直接返回空 Vec，不发起 API 调用）
    /// - top_n: 返回的最大结果数
    fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> impl std::future::Future<Output = Result<Vec<RerankResult>, RerankError>> + Send;
}

#[derive(Debug, Clone, Serialize)]
struct RerankRequest {
    model: String,
    query: String,
    documents: Vec<String>,
    top_n: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct RerankResponse {
    results: Vec<RerankResultRaw>,
}

#[derive(Debug, Clone, Deserialize)]
struct RerankResultRaw {
    index: usize,
    relevance_score: f64,
}

/// OpenRouter Rerank provider
#[derive(Clone)]
pub struct OpenRouterReranker {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
}

impl OpenRouterReranker {
    pub fn new(api_key: String, model: String, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: "https://openrouter.ai/api/v1/rerank".to_string(),
            timeout,
        }
    }

    /// Constructor with custom base_url for testing (mockito)
    pub fn with_base_url(
        api_key: String,
        model: String,
        timeout: Duration,
        base_url: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
            timeout,
        }
    }
}

impl Reranker for OpenRouterReranker {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: documents.to_vec(),
            top_n,
        };

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    RerankError::Timeout(self.timeout)
                } else {
                    RerankError::Network(e.to_string())
                }
            })?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let message = response.text().await.unwrap_or_else(|e| e.to_string());
            return Err(RerankError::Api { status, message });
        }

        let text = response.text().await.map_err(|e| {
            RerankError::ResponseParse(format!("failed to read response body: {e}"))
        })?;

        let parsed: RerankResponse =
            serde_json::from_str(&text).map_err(|e| RerankError::ResponseParse(e.to_string()))?;

        Ok(parsed
            .results
            .into_iter()
            .map(|r| RerankResult {
                index: r.index,
                relevance_score: r.relevance_score,
            })
            .collect())
    }
}

/// 智谱 BigModel Rerank provider
#[derive(Clone)]
pub struct BigModelReranker {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
}

impl BigModelReranker {
    pub fn new(api_key: String, model: String, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: "https://open.bigmodel.cn/api/paas/v4/rerank".to_string(),
            timeout,
        }
    }

    /// Constructor with custom base_url for testing (mockito)
    pub fn with_base_url(
        api_key: String,
        model: String,
        timeout: Duration,
        base_url: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
            timeout,
        }
    }
}

impl Reranker for BigModelReranker {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: documents.to_vec(),
            top_n,
        };

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    RerankError::Timeout(self.timeout)
                } else {
                    RerankError::Network(e.to_string())
                }
            })?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let message = response.text().await.unwrap_or_else(|e| e.to_string());
            return Err(RerankError::Api { status, message });
        }

        let text = response.text().await.map_err(|e| {
            RerankError::ResponseParse(format!("failed to read response body: {e}"))
        })?;

        let parsed: RerankResponse =
            serde_json::from_str(&text).map_err(|e| RerankError::ResponseParse(e.to_string()))?;

        Ok(parsed
            .results
            .into_iter()
            .map(|r| RerankResult {
                index: r.index,
                relevance_score: r.relevance_score,
            })
            .collect())
    }
}

/// 阿里云百炼 (DashScope) Rerank provider
///
/// 接入百炼官方 OpenAI 兼容精排扁平端点
/// `https://dashscope.aliyuncs.com/compatible-api/v1/reranks`，
/// 使用 `qwen3-rerank` 等模型。请求/响应结构与 OpenRouter Rerank 完全兼容。
#[derive(Clone)]
pub struct DashScopeReranker {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
}

impl DashScopeReranker {
    pub fn new(api_key: String, model: String, timeout: Duration) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: "https://dashscope.aliyuncs.com/compatible-api/v1/reranks".to_string(),
            timeout,
        }
    }

    /// Constructor with custom base_url for testing (mockito)
    pub fn with_base_url(
        api_key: String,
        model: String,
        timeout: Duration,
        base_url: String,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
            timeout,
        }
    }
}

impl Reranker for DashScopeReranker {
    async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, RerankError> {
        if documents.is_empty() {
            return Ok(Vec::new());
        }

        let body = RerankRequest {
            model: self.model.clone(),
            query: query.to_string(),
            documents: documents.to_vec(),
            top_n,
        };

        let response = self
            .client
            .post(&self.base_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .timeout(self.timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    RerankError::Timeout(self.timeout)
                } else {
                    RerankError::Network(e.to_string())
                }
            })?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let message = response.text().await.unwrap_or_else(|e| e.to_string());
            return Err(RerankError::Api { status, message });
        }

        let text = response.text().await.map_err(|e| {
            RerankError::ResponseParse(format!("failed to read response body: {e}"))
        })?;

        let parsed: RerankResponse =
            serde_json::from_str(&text).map_err(|e| RerankError::ResponseParse(e.to_string()))?;

        Ok(parsed
            .results
            .into_iter()
            .map(|r| RerankResult {
                index: r.index,
                relevance_score: r.relevance_score,
            })
            .collect())
    }
}

/// Enum dispatch for reranker providers
#[derive(Clone)]
pub enum RerankerProvider {
    OpenRouter(OpenRouterReranker),
    BigModel(BigModelReranker),
    DashScope(DashScopeReranker),
}

impl RerankerProvider {
    pub async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_n: usize,
    ) -> Result<Vec<RerankResult>, RerankError> {
        match self {
            Self::OpenRouter(r) => r.rerank(query, documents, top_n).await,
            Self::BigModel(r) => r.rerank(query, documents, top_n).await,
            Self::DashScope(r) => r.rerank(query, documents, top_n).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_documents(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("document {i}")).collect()
    }

    #[tokio::test]
    async fn openrouter_rerank_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/rerank")
            .match_header("Authorization", "Bearer test-key")
            .match_header("Content-Type", "application/json")
            .with_status(200)
            .with_body(
                r#"{"results":[{"index":1,"relevance_score":0.95},{"index":0,"relevance_score":0.8}]}"#,
            )
            .create_async()
            .await;

        let reranker = OpenRouterReranker::with_base_url(
            "test-key".to_string(),
            "cohere/rerank-v4-fast".to_string(),
            Duration::from_secs(3),
            format!("{}/rerank", server.url()),
        );

        let docs = make_documents(3);
        let results = reranker
            .rerank("test query", &docs, 10)
            .await
            .expect("rerank should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].index, 1);
        assert!((results[0].relevance_score - 0.95).abs() < f64::EPSILON);
        assert_eq!(results[1].index, 0);
        assert!((results[1].relevance_score - 0.8).abs() < f64::EPSILON);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn bigmodel_rerank_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/rerank")
            .match_header("Authorization", "Bearer bm-key")
            .with_status(200)
            .with_body(
                r#"{"id":"req-1","model":"rerank-pro","results":[{"index":2,"relevance_score":0.99},{"index":0,"relevance_score":0.5}],"usage":{"total_tokens":100}}"#,
            )
            .create_async()
            .await;

        let reranker = BigModelReranker::with_base_url(
            "bm-key".to_string(),
            "rerank-pro".to_string(),
            Duration::from_secs(3),
            format!("{}/rerank", server.url()),
        );

        let docs = make_documents(3);
        let results = reranker
            .rerank("test query", &docs, 10)
            .await
            .expect("rerank should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].index, 2);
        assert!((results[0].relevance_score - 0.99).abs() < f64::EPSILON);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn dashscope_rerank_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/reranks")
            .match_header("Authorization", "Bearer ds-key")
            .with_status(200)
            .with_body(
                r#"{"object":"list","results":[{"index":1,"relevance_score":0.95},{"index":0,"relevance_score":0.8}],"model":"qwen3-rerank","id":"x","usage":{"total_tokens":79}}"#,
            )
            .create_async()
            .await;

        let reranker = DashScopeReranker::with_base_url(
            "ds-key".to_string(),
            "qwen3-rerank".to_string(),
            Duration::from_secs(3),
            format!("{}/reranks", server.url()),
        );

        let docs = make_documents(3);
        let results = reranker
            .rerank("test query", &docs, 10)
            .await
            .expect("rerank should succeed");

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].index, 1);
        assert!((results[0].relevance_score - 0.95).abs() < f64::EPSILON);
        assert_eq!(results[1].index, 0);
        assert!((results[1].relevance_score - 0.8).abs() < f64::EPSILON);

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn rerank_empty_documents_returns_empty() {
        let reranker = OpenRouterReranker::new(
            "key".to_string(),
            "model".to_string(),
            Duration::from_secs(3),
        );
        let results = reranker
            .rerank("query", &[], 10)
            .await
            .expect("empty documents should return empty vec");
        assert!(
            results.is_empty(),
            "empty documents should return empty vec"
        );
    }

    #[tokio::test]
    async fn rerank_api_error_degrades() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/rerank")
            .with_status(401)
            .with_body(r#"{"error":"invalid api key"}"#)
            .create_async()
            .await;

        let reranker = OpenRouterReranker::with_base_url(
            "bad-key".to_string(),
            "model".to_string(),
            Duration::from_secs(3),
            format!("{}/rerank", server.url()),
        );

        let docs = make_documents(2);
        let err = reranker
            .rerank("query", &docs, 10)
            .await
            .expect_err("should fail on 401");

        match err {
            RerankError::Api { status, message } => {
                assert_eq!(status, 401);
                assert!(message.contains("invalid api key"));
            }
            other => panic!("expected Api error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn rerank_timeout_error() {
        // Use an unreachable address with a very short timeout to trigger
        // a timeout error. We avoid mockito here because mockito's server
        // responds immediately with headers, causing reqwest to report a
        // body-decode error rather than a connection timeout.
        let timeout = Duration::from_millis(1);
        let reranker = OpenRouterReranker::with_base_url(
            "key".to_string(),
            "model".to_string(),
            timeout,
            // 198.51.100.1 is a documentation-range IP; connection will not complete
            "http://198.51.100.1:1/rerank".to_string(),
        );

        let docs = vec!["document content".to_string()];
        let result = reranker.rerank("query", &docs, 10).await;

        match result {
            Err(RerankError::Timeout(d)) => {
                assert_eq!(d, timeout);
            }
            Err(RerankError::Network(msg)) => {
                // On some platforms or network configs, the error may manifest
                // as a generic network error rather than a specific timeout.
                // Either way, the caller treats it as degradation.
                let _ = msg;
            }
            other => panic!("expected Timeout or Network error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn rerank_response_parse_error() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/rerank")
            .with_status(200)
            .with_body(r#"{"unexpected": "format"}"#)
            .create_async()
            .await;

        let reranker = OpenRouterReranker::with_base_url(
            "key".to_string(),
            "model".to_string(),
            Duration::from_secs(3),
            format!("{}/rerank", server.url()),
        );

        let docs = make_documents(2);
        let err = reranker
            .rerank("query", &docs, 10)
            .await
            .expect_err("should fail on bad JSON");

        match err {
            RerankError::ResponseParse(msg) => {
                assert!(
                    msg.contains("results"),
                    "parse error should mention missing field"
                );
            }
            other => panic!("expected ResponseParse error, got: {other:?}"),
        }
    }
}
