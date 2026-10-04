//! rig OpenAI 兼容客户端构造的统一入口。
//!
//! rig 0.43 的 `OpenAIConfig::client()` 走进程级共享 reqwest 连接池
//! （`rig_reqwest::shared()` 的 LazyLock）。共享池把连接绑定在首次使用的
//! tokio runtime 上：该 runtime 结束后，池内 keep-alive 连接的事件循环随之
//! 死亡，后续请求复用这些连接会永久挂起。集成测试（#[tokio::test] 每测试
//! 一个 runtime）实测会跨用例挂死。因此这里显式为每个客户端创建独享的
//! reqwest transport —— 与 rig 0.39 每个 Client 独立连接池的行为一致。

/// 用独享 reqwest 连接池构建 OpenAI 兼容客户端。
pub fn connect(config: rig::providers::openai::OpenAIConfig) -> rig::providers::openai::OpenAI {
    config.connect(rig::http_client::ReqwestClient::from(reqwest::Client::new()))
}

/// 同 [`connect`]，并显式指定 base URL（生产配置了自定义端点、或测试指向
/// mockito server 时使用）。
pub fn connect_at(
    api_key: impl Into<String>,
    base_url: impl Into<String>,
) -> rig::providers::openai::OpenAI {
    connect(rig::providers::openai::OpenAIConfig {
        base_url: base_url.into(),
        ..rig::providers::openai::OpenAIConfig::new(api_key)
    })
}
