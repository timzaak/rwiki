# Chat Widget 嵌入式 JS 导出 产品需求文档 (PRD)

**状态**: Implemented
**创建时间**: 2026-05-29
**优先级**: P1

---

## 1. 相关用户故事

> 详细故事与验收标准请查看 `docs/user-stories/` 中对应文档。

### 1.1 相关故事

| ID | 标题 | 优先级 | 角色 | 来源 |
|----|------|--------|------|------|
| US-INTG-001 | 嵌入聊天组件到第三方网站 | P0 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-INTG-002 | 定制 Widget 外观 | P1 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-INTG-003 | 管理 Widget 生命周期 | P2 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-CORE-002 | 与知识库进行多轮对话 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-003 | 流式查看回答 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-006 | 在弹窗中使用聊天 | P1 | User | `docs/user-stories/01-user-user-stories.md` |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 3 | 嵌入 Widget、流式查看回答、多轮对话 |
| P1 | 2 | 定制外观、弹窗聊天 |
| P2 | 1 | Widget 生命周期 |

---

## 2. 范围界定

### 2.1 包含功能

- 将现有聊天组件（浮动按钮 + 弹窗对话框）打包为独立 JS 文件（IIFE 格式）
- Widget 通过 Shadow DOM 渲染，与宿主页面样式完全隔离
- 提供 `RWikiChat.init(config)` 全局初始化接口
- 支持外观定制：主题色、标题、按钮位置（左下/右下）、欢迎语
- 界面语言可配置（默认跟随浏览器），并支持运行时通过 `setLocale` 实时切换
- 复用现有聊天交互能力：SSE 流式响应、多轮对话、Markdown 渲染
- 响应式适配：PC 端浮动窗口，移动端全屏覆盖
- 提供 `RWikiChat.destroy()` 销毁接口
- 提供 `RWikiChat.setLocale(locale)` 实时切换语言接口

### 2.2 不包含功能 (Out of Scope)

- 后端 CORS 配置实现（独立任务，本 PRD 仅声明依赖）
- API Token 鉴权实现（依赖 `docs/prd/infrastructure/api-token-auth.md`）
- 同一页面多 Widget 实例支持
- Widget 使用数据统计与分析
- Widget 版本管理与 CDN 分发策略
- 聊天历史持久化（与现有 chat-assistant 一致，会话级保留）
- 新增后端 API 或修改现有接口
- 宿主页面与 Widget 之间的消息通信（postMessage 等）

### 2.3 依赖项

- **后端 CORS 支持**：第三方网站跨域调用 `/api/chat` 的前置条件，需在后端配置允许的域名
- **多频道支持**：Widget 必须在初始化时声明目标频道，频道定义与数据隔离规则见 `docs/prd/integration/support-multiple-website.md`
- **API Token 鉴权**（生产环境推荐）：防止未授权的 API 访问，依赖 `api-token-auth` PRD
- **现有聊天组件**：复用 `frontend/src/components/chat/` 下的组件和 `chat-store`
- **构建工具**：Vite library mode（已有 Vite 依赖，无需新增）

---

## 3. 需求概述

### 3.1 功能描述

提供一个可嵌入的 JS Widget，让第三方网站通过引入单个 JS 文件并传入少量配置，即可在页面中展示 RWiki 知识库聊天功能。网站访客无需跳转即可提问并获得流式回答。Widget 通过 Shadow DOM 实现样式隔离，不与宿主页面产生样式冲突。

### 3.2 关键特性

- **单文件分发**：一个 JS 文件包含全部依赖（React、样式、图标），无外部依赖
- **零冲突嵌入**：Shadow DOM 隔离，不影响宿主页面样式，也不受宿主样式影响
- **配置化接入**：`apiUrl` 必填，主题色/标题/位置/欢迎语可选
- **复用现有能力**：SSE 流式、多轮对话、Markdown 渲染均复用已有实现

---

## 4. 业务规则与状态

### 4.1 业务规则

- **必填配置**：`apiUrl` 与目标频道标识（`channelId`）均为必填项，任一缺失时 Widget 不渲染并在控制台报错
- **频道绑定**：Widget 的聊天、推荐问题、反馈请求均绑定到初始化时声明的频道；详见 `docs/prd/integration/support-multiple-website.md`
- **样式隔离**：所有 Widget 样式限定在 Shadow DOM 内，不泄漏到宿主页面
- **依赖内联**：React、ReactDOM、Zustand、react-markdown 等全部打包到单文件中，不依赖宿主页面的任何全局库
- **浏览器兼容**：Chrome 53+, Firefox 63+, Safari 10+（Shadow DOM 最低版本），不支持 IE11
- **体积约束**：构建产物 gzip 后控制在 300KB 以内

### 4.2 关键状态与异常

- **初始化失败**：`apiUrl` 或 `channelId` 缺失/格式错误 → 控制台报错，不渲染 Widget
- **频道不可用**：`channelId` 未在后端配置中定义 → 访客发送问题后提示"频道不存在或不可用"（详见多频道 PRD）
- **后端不可达**：发送消息时连接失败 → 聊天窗口显示"无法连接服务"提示
- **SSE 中断**：流式响应中断 → 已显示内容保留，提示"回答生成中断"
- **宿主页面 React 冲突**：不冲突，因为 Widget 自带完整 React 实例

---

## 5. 功能需求

### 5.1 核心需求

1. **Widget 构建与分发**
   - 使用 Vite library mode 构建 IIFE 格式单文件
   - CSS 通过 `?inline` 内联为 JS 字符串，运行时注入 Shadow DOM
   - 构建产物为单个 `rwiki-chat.js` 文件

2. **初始化接口**
   - 暴露全局对象 `RWikiChat`
   - `init(config)` 方法创建 Shadow DOM 容器、注入样式、挂载 React 应用
   - `destroy()` 方法移除 Widget DOM 并清理资源
   - `setLocale(locale)` 方法在不卸载 Widget 的前提下实时切换界面语言（重新渲染，保留当前对话）

3. **配置接口**
   - `apiUrl`（必填）：后端 API 地址
   - `channelId`（必填）：目标频道标识，必须为后端已配置的频道；缺失或为空时不渲染 Widget
   - `primaryColor`（可选）：主题强调色，默认 `#3b82f6`
   - `title`（可选）：对话框标题，默认"Chat Assistant"
   - `position`（可选）：浮动按钮位置，`right`（默认）或 `left`
   - `welcomeMessage`（可选）：首次打开时的欢迎语
   - `locale`（可选）：界面语言，接受 BCP-47 标签，解析为受支持语言（`en`、`zh-CN`，任意中文变体归为 `zh-CN`，其余回退 `en`），默认取 `navigator.language`；运行时可用 `setLocale` 切换

4. **聊天交互**
   - 复用现有浮动按钮 + 弹窗对话框的交互模式
   - SSE 流式响应，逐字渲染回答
   - 多轮对话，会话内保留上下文
   - Markdown 格式渲染回答内容

5. **响应式适配**
   - PC 端：浮动窗口
   - 移动端：全屏覆盖

### 5.2 验收目标

- 第三方网站通过以下代码即可获得可用的聊天 Widget：
  ```html
  <script src="rwiki-chat.js"></script>
  <script>RWikiChat.init({ apiUrl: 'https://rwiki.example.com', channelId: 'help-center' })</script>
  ```
- Widget 在包含 Bootstrap、Tailwind、Ant Design 等常见 UI 框架的页面中无样式冲突
- SSE 流式响应在跨域场景正常工作
- 构建产物为单个 JS 文件，gzip 后不超过 300KB
- 移动端自动切换为全屏模式

---

## 6. API 相关约束

**适用性**: 适用

- **能力范围**：Widget 调用现有 `/api/chat` SSE 接口，不新增后端接口
- **跨域依赖**：后端必须配置 CORS 允许第三方域名访问 `/api/chat`，否则 Widget 在跨域场景不可用
- **鉴权建议**：生产环境建议配合 API Token 鉴权使用（依赖 `api-token-auth`），开发/测试环境可不鉴权
- **兼容性**：不修改现有接口的行为和参数

---

## 7. 前端/交互约束

**适用性**: 适用

### 7.1 页面入口

- 宿主页面通过 `<script>` 标签引入 JS 文件
- `RWikiChat.init()` 调用后在页面 body 末尾注入 Shadow DOM 容器

### 7.2 关键交互

- **浮动按钮**：默认右下角，可配置为左下角；点击打开/关闭对话框
- **对话框**：PC 端为浮动窗口，移动端（屏幕宽度 < 768px）自动全屏覆盖
- **聊天输入**：多行文本输入，Enter 发送，Shift+Enter 换行
- **流式渲染**：回答区域实时显示生成内容，支持 Markdown 格式和代码高亮
- **外观定制**：主题色影响浮动按钮、对话框标题栏、发送按钮等强调色元素

### 7.3 状态反馈

- 初始化中：无可见状态（同步创建）
- 聊天生成中：打字动画指示
- 连接失败：聊天窗口内显示错误提示
- 配置错误：控制台报错，不渲染 Widget

---

## 8. 已确认决策

- **分发格式**：IIFE 单文件，不使用 ESM 或 UMD
- **样式隔离**：Shadow DOM + CSS 内联注入，不使用 CSS 前缀或 CSS Modules
- **构建工具**：Vite library mode，复用项目已有 Vite 依赖
- **组件复用**：复用现有 `chat/` 目录下的组件，需解耦 TanStack Router 依赖
- **CORS 依赖**：后端 CORS 配置为前置依赖，不在本 PRD 范围内实现
- **鉴权方式**：生产环境推荐 API Token，开发环境可免鉴权
- **不支持 IE11**：Shadow DOM 在 IE11 不可用
- **不支持多实例**：同一页面仅支持一个 Widget 实例
- **频道必填**：`channelId` 与 `apiUrl` 同为必填初始化项；推荐问题采用频道强控——频道已配置推荐问题时，Widget 本地推荐问题配置被忽略（见多频道 PRD）

---

## 9. 参考资料

- 用户故事：`docs/user-stories/02-website-integrator-user-stories.md`
- 相关用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-002, 003, 006）
- 角色定义：`docs/user-stories/_roles.md`
- 技术预研：`.ai/tech-research/chat-widget-embeddable-js.md`
- 相关 PRD：`docs/prd/chat/chat-assistant.md`
- 依赖 PRD：`docs/prd/infrastructure/api-token-auth.md`、`docs/prd/integration/support-multiple-website.md`
