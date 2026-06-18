# API Token 鉴权 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-29
**优先级**: P0

---

## 1. 相关用户故事

> 本功能为基础设施安全特性，无直接对应的用户故事。鉴权范围覆盖现有文档管理相关用户故事的 API 访问。

### 1.1 相关故事

| ID | 标题 | 优先级 | 影响 |
|----|------|--------|------|
| US-CORE-001 | 上传 xlsx 文件到知识库 | P0 | 鉴权保护 |
| US-CORE-004 | 查看已上传文档列表 | P1 | 鉴权保护 |
| US-CORE-009 | 管理文档发布状态 | P1 | 鉴权保护 |

### 1.2 优先级汇总

| 优先级 | 说明 |
|--------|------|
| P0 | 文档接口安全为上线前必要条件 |

---

## 2. 范围界定

### 2.1 包含功能

- **Bearer Token 认证**：调用方在请求头中携带 API Token 进行身份验证
- **IP range allow list**：可选配置允许访问受保护接口的来源 IP 网段，降低 Token 被爆破或泄露后的可用性
- **配置管理**：API Token 通过配置文件或环境变量设置
- **强制配置**：未配置 API Token 时服务拒绝启动
- **鉴权范围**：受保护路由组（`doc_router`）覆盖文档管理操作（上传、列表、删除、发布、取消发布）、反馈查询（`GET /api/chat/feedback`）和评估端点（`POST /api/eval/query`，内部评估工具）；`/health`、`POST /api/chat`、`GET /api/chat/suggestions` 和反馈提交（`POST /api/chat/feedback`）保持公开

### 2.2 不包含功能 (Out of Scope)

- JWT 签发/刷新/吊销机制
- 多 Token 管理
- 基于角色的权限控制
- Token 过期和轮换机制
- 前端登录/认证 UI
- Chat 接口鉴权（保持公开）

### 2.3 依赖项

- 现有 axum Web 框架中间件机制
- 现有配置管理（AppConfig + 环境变量覆盖）
- 现有 OpenAPI SecurityScheme 声明

---

## 3. 需求概述

### 3.1 功能描述

为文档管理接口增加 API Token 鉴权，防止未授权访问。API Token 为静态字符串，通过配置文件或环境变量注入。所有文档管理操作需要有效 Token 才能访问，聊天和健康检查接口保持公开。

### 3.2 关键特性

- **静态 Token**：单 Token 配置，适用于内部工具场景
- **配置驱动**：支持配置文件和环境变量两种方式，环境变量优先
- **强制安全**：未配置 Token 时服务拒绝启动
- **路由级鉴权**：仅文档路由受保护，其他路由不受影响

---

## 4. 业务规则与状态

### 4.1 业务规则

- **鉴权范围**：受保护路由组（`doc_router`）覆盖文档管理操作（上传、列表、删除、发布、取消发布）、反馈查询（`GET /api/chat/feedback`）和评估端点（`POST /api/eval/query`，内部评估工具）；`/health`、`POST /api/chat`、`GET /api/chat/suggestions` 和反馈提交（`POST /api/chat/feedback`）保持公开
- **公开路由**：聊天接口和健康检查接口保持公开，无需 Token
- **Token 传递方式**：`Authorization: Bearer <token>` 请求头
- **强制配置**：API Token 为必填配置，未配置时服务拒绝启动
- **IP allow list**：`allowed_ip_ranges` 为空时不限制来源 IP；非空时，TCP peer IP 必须命中其中一个 CIDR 网段
- **错误响应**：Token 缺失或无效时返回统一 401 错误，不区分具体原因

### 4.2 关键状态与异常

- **未配置 Token**：服务启动时校验，为空则 panic 并提示配置方法
- **Token 无效**：返回 401 未授权错误
- **Token 缺失**：返回 401 未授权错误（与 Token 无效相同响应，避免信息泄露）
- **来源 IP 不允许**：返回 401 未授权错误（与 Token 错误相同响应，避免信息泄露）

---

## 5. 功能需求

### 5.1 核心需求

1. **Token 配置管理**
   - 支持通过配置文件设置 API Token
   - 支持通过环境变量覆盖配置文件中的 Token
   - 启动时校验 Token 非空
   - 支持通过配置文件或环境变量设置 `allowed_ip_ranges`

2. **请求鉴权**
   - 从请求头提取 Bearer Token
   - 与配置值比对验证
   - 当配置 IP allow list 时，从 Axum 连接信息读取 TCP peer IP 并判断是否允许
   - 无效或缺失时返回 401

3. **路由保护**
   - 文档管理路由组（上传、列表、删除、发布、取消发布）需鉴权
   - 聊天和健康检查路由不受影响

### 5.2 验收目标

- 未携带 Token 访问文档接口返回 401
- 携带无效 Token 访问文档接口返回 401
- 携带有效 Token 访问文档接口正常处理
- 聊天接口和健康检查接口无需 Token 即可访问
- 未配置 API Token 时服务拒绝启动
- 未配置 `allowed_ip_ranges` 时保持现有 Token 鉴权行为
- 配置 `allowed_ip_ranges` 后，TCP peer IP 不在允许网段内的请求返回 401
- OpenAPI 文档正确反映鉴权要求

---

## 6. API 相关约束

**适用性**: 适用

- **能力范围**：受保护路由组（文档管理操作、`GET /api/chat/feedback`、`POST /api/eval/query`）需要 Bearer Token 鉴权
- **鉴权方式**：`Authorization: Bearer <token>` 请求头
- **访问控制**：无 Token、Token 无效或来源 IP 不允许时返回 401，不区分具体原因
- **公开接口**：聊天接口和健康检查接口无需鉴权
- **OpenAPI 契约**：文档管理操作的 OpenAPI 文档需标注安全要求

---

## 7. 前端/交互约束

**适用性**: 不适用

本功能为后端 API 鉴权机制，前端不涉及上传 xlsx 操作（由外部工具/脚本调用），无前端变更。

---

## 8. 已确认决策

- **Token 类型**：静态 API Token，非 JWT，适用于内部工具场景
- **配置方式**：配置文件 + 环境变量覆盖（与现有 LLM API Key 模式一致）
- **鉴权范围**：受保护路由组包括 upload / list / delete / publish / unpublish、`GET /api/chat/feedback` 和 `POST /api/eval/query`；`/health`、`POST /api/chat`、`GET /api/chat/suggestions` 和 `POST /api/chat/feedback` 公开
- **强制配置**：api_token 为必填项，未配置时服务拒绝启动
- **错误响应**：Token 缺失和无效返回相同 401 响应（安全最佳实践）

---

## 9. 参考资料

- 关联 PRD：`docs/prd/document/document-lifecycle.md`（文档发布状态）
