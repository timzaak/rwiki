# Chat Widget 嵌入式 JS 导出 产品需求文档 (PRD)

**状态**: Implemented（2026-11-10 扩展：右侧推挤式停靠侧栏显示形态，US-INTG-018/US-CORE-047；2026-11-09 扩展：`ask` / `setChannel` 全局接口，US-INTG-016/017）
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
| US-INTG-016 | 运行时切换 Widget 频道 | P2 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-INTG-017 | 把检索词交给 Widget 直接提问 | P1 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-INTG-018 | 为 Widget 选择显示形态 | P1 | Website Integrator | `docs/user-stories/02-website-integrator-user-stories.md` |
| US-CORE-002 | 与知识库进行多轮对话 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-003 | 流式查看回答 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-006 | 在弹窗中使用聊天 | P1 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-047 | 在右侧停靠侧栏中使用聊天 | P1 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-048 | 查看历史会话列表并来回切换回看 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-049 | 历史会话长期保留在本地浏览器 | P0 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-050 | 在历史会话中继续对话 | P1 | User | `docs/user-stories/01-user-user-stories.md` |
| US-CORE-051 | 删除历史会话 | P1 | User | `docs/user-stories/01-user-user-stories.md` |

### 1.2 优先级汇总

| 优先级 | 数量 | 关键故事 |
|--------|------|----------|
| P0 | 5 | 嵌入 Widget、流式查看回答、多轮对话、历史列表切换回看、本地长期保留 |
| P1 | 6 | 定制外观、弹窗聊天、选择显示形态、侧栏使用、历史续聊、删除历史 |
| P2 | 2 | Widget 生命周期、运行时切换频道 |

---

## 2. 范围界定

### 2.1 包含功能

- 将现有聊天组件（浮动按钮 + 对话面板）打包为独立 JS 文件（IIFE 格式）
- Widget 通过 Shadow DOM 渲染，与宿主页面样式隔离（隔离边界见 §4.1）
- 提供 `RWikiChat.init(config)` 全局初始化接口
- 支持外观定制：主题色、标题、按钮位置（左下/右下）、欢迎语
- 显示形态可配置：弹窗（默认，现状行为不变）与右侧推挤式停靠侧栏二选一
- 侧栏形态：面板固定视口右缘、全高；打开时宿主页面文档流内容整体左移让出空间，关闭与销毁后复原且无残留
- 侧栏打开期间宿主页面内容保持可见、可交互（无变暗遮罩）
- 侧栏形态在移动端回退为全屏覆盖，不推挤宿主内容
- 界面语言可配置（默认跟随浏览器），并支持运行时通过 `setLocale` 实时切换
- 复用现有聊天交互能力：SSE 流式响应、多轮对话、Markdown 渲染
- 响应式适配：PC 端浮动窗口（弹窗形态）或推挤侧栏（侧栏形态），移动端全屏覆盖
- 提供 `RWikiChat.destroy()` 销毁接口
- 提供 `RWikiChat.setLocale(locale)` 实时切换语言接口
- 提供 `RWikiChat.ask(query)` 接口：打开当前显示形态的面板并把 query 作为一轮用户提问自动发出（复用既有隐式中断语义；空串/空白被忽略）
- 提供 `RWikiChat.setChannel(channelId)` 运行时切换频道接口：切换即开启新会话（中止在途回答，当前会话归档进其频道的浏览器本地历史后清空工作区，再按新频道重新渲染）；频道集合不变时为 no-op（不误清会话）

### 2.2 不包含功能 (Out of Scope)

- 后端 CORS 配置实现（独立任务，本 PRD 仅声明依赖）
- API Token 鉴权实现（依赖 `docs/prd/infrastructure/api-token-auth.md`）
- 同一页面多 Widget 实例支持
- Widget 使用数据统计与分析
- Widget 版本管理与 CDN 分发策略
- 聊天历史的服务端持久化与跨设备同步（历史仅存于访客本地浏览器，按频道保留直到手动清除，见 `chat/frontend-user-history.md`）
- 新增后端 API 或修改现有接口（显示形态为纯前端行为）
- 宿主页面与 Widget 之间的消息通信（postMessage 等）
- 左侧停靠侧栏：`position` 配置只控制浮动按钮位置（左下/右下），不延伸为停靠侧
- 针对宿主页面特定元素（如 `position: fixed` 定位元素、独立滚动容器）的推挤适配：此类元素不随文档流位移为已知局限，仅通过配置文档说明并建议使用默认弹窗形态
- docs-web 站点自身切换显示形态：任何宿主（含 docs-web）后续可经同一配置 opt-in，不在本 PRD 范围内

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

显示形态二选一：默认右下角弹窗（现状），或右侧推挤式停靠侧栏——后者贴近主流 AI QA 文档站形态，问答面板与正文并排，访客可边读边问，面板打开时宿主内容整体左移让位而非被遮挡。

### 3.2 关键特性

- **单文件分发**：一个 JS 文件包含全部依赖（React、样式、图标），无外部依赖
- **零冲突嵌入**：Shadow DOM 隔离，不影响宿主页面样式，也不受宿主样式影响（§4.1 例外除外）
- **配置化接入**：`apiUrl` 必填，主题色/标题/位置/欢迎语/显示形态可选
- **形态可选、默认不变**：不配置显示形态的既有宿主行为与现状完全一致，零改动兼容
- **复用现有能力**：SSE 流式、多轮对话、Markdown 渲染均复用已有实现，两种形态下聊天语义完全一致

---

## 4. 业务规则与状态

### 4.1 业务规则

- **必填配置**：`apiUrl` 与目标频道标识（`channelId`）均为必填项，任一缺失时 Widget 不渲染并在控制台报错
- **频道绑定**：Widget 的聊天、推荐问题、反馈请求均绑定到初始化时声明的频道；详见 `docs/prd/integration/support-multiple-website.md`
- **样式隔离（含侧栏例外）**：Widget 自身视觉样式限定在 Shadow DOM 内，不泄漏到宿主页面。范围受限例外：侧栏形态打开时，允许向宿主文档根层级注入文档流腾位所需的最小布局样式（仅限文档根元素与过渡效果），随侧栏关闭与 `destroy()` 移除；注入样式不得针对宿主特定元素选择器，除此之外不得向宿主页面注入任何样式
- **形态语义**：显示形态只改变面板呈现与宿主布局，聊天、推荐问题、反馈、频道、多语言等语义与接口在两种形态下完全一致
- **依赖内联**：React、ReactDOM、Zustand、react-markdown 等全部打包到单文件中，不依赖宿主页面的任何全局库
- **浏览器兼容**：Chrome 53+, Firefox 63+, Safari 10+（Shadow DOM 最低版本），不支持 IE11
- **体积约束**：构建产物 gzip 后控制在 300KB 以内（显示形态新增不引入新依赖）

### 4.2 关键状态与异常

- **初始化失败**：`apiUrl` 或 `channelId` 缺失/格式错误 → 控制台报错，不渲染 Widget
- **显示形态配置非法**：传入不支持的显示形态取值 → 控制台报错，不渲染 Widget（沿现有校验模式）
- **频道不可用**：`channelId` 未在后端配置中定义 → 访客发送问题后提示"频道不存在或不可用"（详见多频道 PRD）
- **后端不可达**：发送消息时连接失败 → 聊天窗口显示"无法连接服务"提示
- **SSE 中断**：流式响应中断 → 已显示内容保留，提示"回答生成中断"
- **宿主页面 React 冲突**：不冲突，因为 Widget 自带完整 React 实例
- **侧栏关闭/销毁残留**：侧栏收起或 `destroy()` 后宿主布局必须复原，不残留注入样式；快速重复开关不得产生残留（幂等清理）
- **宿主 fixed 元素局限**：宿主页面 `position: fixed` 定位元素、独立滚动容器不随文档流位移，推挤后可能视觉错位——已知局限，配置文档说明并建议此类宿主使用默认弹窗形态（两种形态并存即回退手段）

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
   - `destroy()` 方法移除 Widget DOM 并清理资源；销毁前当前会话归档进其频道的浏览器本地历史（访客本地数据不因销毁丢失）；侧栏打开状态下销毁时同步移除注入宿主的布局样式并复原宿主布局
   - `setLocale(locale)` 方法在不卸载 Widget 的前提下实时切换界面语言（重新渲染，保留当前对话）
   - `ask(query)` 方法（2026-11-09，US-INTG-017）：打开当前显示形态的面板并把 trim 后的 query 交给组件内发送链路作为一轮用户提问；侧栏形态下打开侧栏并让出宿主空间；已在流的回答按既有隐式中断语义收尾；init 前调用报控制台错误。宿主页（如 docs-web 搜索的「用 AI 回答」）由此把检索词升级为 AI 问答
   - `setChannel(channelId)` 方法（2026-11-09，US-INTG-016）：参数校验与 `init` 的 channelId 一致（string | string[]，归一化去重）；频道集合不变时直接返回；变化时先中止在途回答，把当前会话归档进其频道的浏览器本地历史（见 `chat/frontend-user-history.md`），再清空工作区并以新频道重新渲染（与 setLocale 同模式）；init 前调用报控制台错误

3. **配置接口**
   - `apiUrl`（必填）：后端 API 地址
   - `channelId`（必填）：目标频道标识，必须为后端已配置的频道；缺失或为空时不渲染 Widget
   - `primaryColor`（可选）：主题强调色，默认 `#3b82f6`
   - `title`（可选）：对话框标题，默认"Chat Assistant"
   - `position`（可选）：浮动按钮位置，`right`（默认）或 `left`；语义不随显示形态改变（不延伸为停靠侧）
   - `welcomeMessage`（可选）：首次打开时的欢迎语
   - `locale`（可选）：界面语言，接受 BCP-47 标签，解析为受支持语言（`en`、`zh-CN`，任意中文变体归为 `zh-CN`，其余回退 `en`），默认取 `navigator.language`；运行时可用 `setLocale` 切换
   - `displayMode`（可选，2026-11-10，US-INTG-018）：显示形态，`modal`（默认，右下角弹窗，现状行为不变）或 `sidePanel`（右侧推挤侧栏）；非法取值时控制台报错且不渲染 Widget

4. **显示形态行为**
   - 弹窗形态：现状行为完全不变（可拖拽浮动窗口、变暗遮罩）
   - 侧栏形态：全高面板停靠视口右缘，从右缘滑入/滑出；不可拖拽、无变暗遮罩（推挤语义下宿主内容保持可见可交互）；打开期间右下角浮动按钮自动隐藏，经侧栏头部关闭图标收起
   - 宿主推挤：侧栏打开时向宿主文档根注入腾位样式使文档流内容整体左移（腾位宽度与面板宽度一致），关闭与 `destroy()` 时移除
   - 侧栏默认宽度与现有弹窗一致量级

5. **聊天交互**
   - 复用现有浮动按钮 + 面板的交互模式
   - SSE 流式响应，逐字渲染回答
   - 多轮对话，会话内保留上下文
   - Markdown 格式渲染回答内容
   - 两种形态下聊天能力完全一致

6. **响应式适配**
   - PC 端：弹窗形态为浮动窗口，侧栏形态为右侧推挤侧栏
   - 移动端：两种形态均全屏覆盖，不推挤宿主内容（沿现有 `max-sm` 断点，< 640px）

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
- 配置侧栏形态后：侧栏打开时宿主文档流内容整体左移让位、页面内容保持可见可交互；关闭与 `destroy()` 后宿主布局复原，无注入残留
- 未配置显示形态的既有嵌入代码行为与现状完全一致（零改动兼容）
- `ask()` 在两种形态下均打开当前形态的面板并自动发出提问
- 两种形态下聊天、推荐问题、反馈、频道、多语言行为一致

---

## 6. API 相关约束

**适用性**: 适用

- **能力范围**：Widget 调用现有 `/api/chat` SSE 接口，不新增后端接口；显示形态为纯前端行为，不触达任何后端 API
- **跨域依赖**：后端必须配置 CORS 允许第三方域名访问 `/api/chat`，否则 Widget 在跨域场景不可用
- **鉴权建议**：生产环境建议配合 API Token 鉴权使用（依赖 `api-token-auth`），开发/测试环境可不鉴权
- **兼容性**：不修改现有接口的行为和参数

---

## 7. 前端/交互约束

**适用性**: 适用

### 7.1 页面入口

- 宿主页面通过 `<script>` 标签引入 JS 文件
- `RWikiChat.init()` 调用后在页面 body 末尾注入 Shadow DOM 容器；侧栏形态打开时另注入宿主文档根布局样式（范围见 §4.1）

### 7.2 关键交互

- **浮动按钮**：默认右下角，可配置为左下角；点击打开面板（弹窗或侧栏，随配置）；弹窗打开时按钮隐藏（现状），侧栏打开时按钮同样隐藏
- **弹窗形态**：PC 端为可拖拽浮动窗口 + 变暗遮罩，移动端（屏幕宽度 < 640px，沿现有 `max-sm` 断点）自动全屏覆盖（现状不变）
- **侧栏形态**：全高面板停靠视口右缘、右缘滑入动画、不可拖拽、无变暗遮罩；打开时宿主内容整体左移让位；经侧栏头部关闭图标收起，收起时宿主内容复位；移动端（屏幕宽度 < 640px）全屏覆盖，不推挤
- **聊天输入**：多行文本输入，Enter 发送，Shift+Enter 换行
- **流式渲染**：回答区域实时显示生成内容，支持 Markdown 格式和代码高亮
- **外观定制**：主题色影响浮动按钮、面板标题栏、发送按钮等强调色元素
- **宿主适配说明**：侧栏形态对含大量 `position: fixed` 元素或独立滚动容器的宿主页面可能产生视觉错位，配置文档须说明该局限并建议此类宿主使用默认弹窗形态

### 7.3 状态反馈

- 初始化中：无可见状态（同步创建）
- 侧栏开合：右缘滑入/滑出过渡动画，宿主内容位移同步过渡
- 聊天生成中：打字动画指示
- 连接失败：聊天窗口内显示错误提示
- 配置错误：控制台报错，不渲染 Widget

---

## 8. 已确认决策

- **分发格式**：IIFE 单文件，不使用 ESM 或 UMD
- **样式隔离**：Shadow DOM + CSS 内联注入，不使用 CSS 前缀或 CSS Modules（隔离边界含侧栏文档根腾位例外，见 §4.1）
- **构建工具**：Vite library mode，复用项目已有 Vite 依赖
- **组件复用**：复用现有 `chat/` 目录下的组件，需解耦 TanStack Router 依赖
- **CORS 依赖**：后端 CORS 配置为前置依赖，不在本 PRD 范围内实现
- **鉴权方式**：生产环境推荐 API Token，开发环境可免鉴权
- **不支持 IE11**：Shadow DOM 在 IE11 不可用
- **不支持多实例**：同一页面仅支持一个 Widget 实例
- **频道必填**：`channelId` 与 `apiUrl` 同为必填初始化项；推荐问题采用频道强控——频道已配置推荐问题时，Widget 本地推荐问题配置被忽略（见多频道 PRD）
- **显示形态**：弹窗（默认，现状行为不变）与右侧推挤式停靠侧栏二选一，经 init 配置选择，完全向后兼容；`ask()` 等既有全局接口自动路由到当前形态
- **侧栏收起交互**：侧栏打开期间右下角浮动按钮自动隐藏（复用弹窗行为），经侧栏头部关闭图标收起；浮动按钮定位为打开入口，非收起切换
- **侧栏停靠侧**：仅停靠视口右缘；`position` 配置只控制浮动按钮位置（左下/右下），不延伸为停靠侧，左侧停靠不在范围内

---

## 9. 参考资料

- 用户故事：`docs/user-stories/02-website-integrator-user-stories.md`
- 相关用户故事：`docs/user-stories/01-user-user-stories.md`（US-CORE-002, 003, 006, 047）
- 角色定义：`docs/user-stories/_roles.md`
- 技术预研：`.ai/tech-research/chat-widget-embeddable-js.md`、`.ai/tech-research/chatWidget.md`
- 相关 PRD：`docs/prd/chat/chat-assistant.md`
- 依赖 PRD：`docs/prd/infrastructure/api-token-auth.md`、`docs/prd/integration/support-multiple-website.md`
