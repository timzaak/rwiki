# 可观测性 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-30
**优先级**: P2
**权威范围**: tracing/span 输出、OTLP 配置、shutdown flush

---

## 1. 范围界定

包含：

- 后端 tracing/span 输出。
- OTLP gRPC exporter 配置和启用条件。
- LLM 调用相关 span 的导出。
- 服务关闭时 flush 未发送 span。

不包含：

- Metrics 导出。
- 前端或浏览器 tracing。
- 自定义业务埋点清单。
- 多 tracing 后端同时导出。
- 非 OTLP 协议支持。

## 2. 业务规则

- OTLP 由 `[otel]` 配置段控制。
- `endpoint` 为空或未配置时，不启用 OTLP 导出。
- `service_name` 默认值为 `rwiki-backend`，可配置覆盖。
- 鉴权 token 作为 OTLP gRPC metadata header 传递。
- OTLP 初始化失败属于配置错误，应启动失败并给出明确错误。
- 运行时 span 导出失败不得影响业务请求。
- 服务 graceful shutdown 时必须 flush pending spans。

## 3. 验收目标

- 配置有效 OTLP endpoint 后，应用 span 可导出到目标 tracing 后端。
- 未配置 OTLP 时，应用行为与普通日志模式一致。
- LLM 调用 span 能包含模型、延迟、token 等 provider 可提供的信息。
- Ctrl+C 或正常停止时 pending spans 被 flush。

## 4. 参考资料

- 聊天助手：`/docs/prd/chat/chat-assistant.md`
- 模型 Provider：`/docs/prd/infrastructure/model-providers.md`
