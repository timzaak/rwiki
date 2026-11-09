# Website Integrator 用户故事

> 角色定义以 `docs/user-stories/_roles.md` 为准。

## 故事 1：嵌入聊天组件到第三方网站 [US-INTG-001]

**优先级**: P0

**【用户故事】**
**作为**：Website Integrator
**我希望**：通过引入一个 JS 文件并调用初始化函数，声明目标频道后在我的网站中嵌入 RWiki 聊天组件
**从而**：我的网站访客可以直接在页面上向所属频道知识库提问，无需跳转

**【验收标准】**

**场景 1：成功嵌入并使用**
```gherkin
Given 网站已通过 <script> 标签引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: 'https://rwiki.example.com', channelId: 'channel-a' })
Then 页面右下角出现浮动聊天按钮
And 点击按钮后弹出聊天对话框，可输入问题并获得流式回答
And 访客的提问只检索频道 channel-a 下的文档
```

**场景 2：缺少必填配置**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init() 未传入 apiUrl 或 channelId
Then 控制台输出错误提示"apiUrl is required"或"channelId is required"
And 页面上不渲染任何 Widget 元素
```

**场景 3：后端不可达**
```gherkin
Given Widget 已初始化，但 apiUrl 指向不可达的服务
When 用户输入问题并发送
Then 聊天窗口显示"无法连接服务，请检查配置或稍后重试"
And 已发送的消息保留在聊天窗口中
```

**场景 4：传入未配置的频道**
```gherkin
Given 后端未配置频道 "channel-unknown"
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-unknown' }) 后访客发送问题
Then 访客收到"频道不存在或不可用"的提示
```

---

## 故事 2：定制 Widget 外观 [US-INTG-002]

**优先级**: P1

**【用户故事】**
**作为**：Website Integrator
**我希望**：通过配置参数定制聊天组件的颜色、标题、位置等视觉元素
**从而**：组件的视觉风格与我的网站设计保持一致

**【验收标准】**

**场景 1：配置主题色和标题**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', primaryColor: '#e74c3c', title: '帮助中心' })
Then 浮动按钮和对话框的强调色使用 #e74c3c
And 对话框标题显示"帮助中心"
```

**场景 2：配置按钮位置**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', position: 'left' })
Then 浮动按钮出现在页面左下角
And 点击后对话框从左侧弹出
```

**场景 3：配置欢迎语**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', welcomeMessage: '有什么可以帮您？' })
Then 首次打开对话框时，聊天区域显示欢迎语"有什么可以帮您？"
```

---

## 故事 3：管理 Widget 生命周期 [US-INTG-003]

**优先级**: P2

**【用户故事】**
**作为**：Website Integrator
**我希望**：在运行时动态销毁或重新初始化 Widget
**从而**：我可以根据业务逻辑控制 Widget 的显隐（如用户登录后才显示）

**【验收标准】**

**场景 1：销毁 Widget**
```gherkin
Given Widget 已初始化并渲染在页面上
When 调用 RWikiChat.destroy()
Then 页面上的浮动按钮和对话框完全移除
And 宿主页面的 DOM 恢复到 Widget 注入前的状态
```

**场景 2：重新初始化**
```gherkin
Given Widget 已被销毁
When 再次调用 RWikiChat.init({ apiUrl: '...' })
Then Widget 重新渲染在页面上
And 新的配置参数生效
```

---

## 故事 4：在 Widget 中配置推荐问题 [US-INTG-004]

**优先级**: P2

**【用户故事】**
**作为**：Website Integrator
**我希望**：在嵌入 Widget 时通过配置参数传入推荐问题，让访客看到与我业务相关的引导问题
**从而**：访客无需构思问题即可快速开始对话，提高 Widget 的首次使用率

> 频道强控规则：当目标频道已在后端配置推荐问题时，Widget 本地传入的推荐问题配置被忽略，统一以服务端频道配置为准（见 `docs/prd/integration/support-multiple-website.md`）。

**【验收标准】**

**场景 1：频道未配置推荐问题时使用 Widget 本地配置**
```gherkin
Given 后端为目标频道未配置推荐问题
And 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-a', suggestedQuestions: { default: ['如何注册', '支持哪些支付方式'], en: ['How to register', 'Payment methods'] } })
Then Widget 打开后在空状态展示匹配访客浏览器语言的推荐问题按钮
And 访客点击按钮后直接发送问题并获得流式回答
```

**场景 2：频道已配置推荐问题时以频道配置为准**
```gherkin
Given 后端为目标频道已配置推荐问题
And 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-a', suggestedQuestions: ['本地问题 A', '本地问题 B'] })
Then Widget 打开后展示的是频道配置的推荐问题，而非本地传入的推荐问题
```

**场景 3：未配置推荐问题时 Widget 正常工作**
```gherkin
Given 后端为目标频道未配置推荐问题
And 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-a' }) 未传入 suggestedQuestions
Then Widget 正常渲染，空状态仅显示欢迎消息
And 访客可手动输入问题正常对话
```

---

## 故事 5：在 Widget 中指定频道 [US-INTG-006]

**优先级**: P0

**【用户故事】**
**作为**：Website Integrator
**我希望**：在初始化 Widget 时传入目标频道，让访客访问到对应频道的知识库
**从而**：我的网站访客只能看到属于我的频道的内容，而不会看到其他频道的数据

**【验收标准】**

**场景 1：带频道标识的 Widget 正常对话**
```gherkin
Given 管理员已为频道 "channel-a" 上传文档并发布
And 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-a' })
Then 访客在 Widget 中的提问只检索频道 channel-a 下的文档
And 访客获得基于该频道知识库的回答
```

**场景 2：未传频道标识时 Widget 不渲染**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...' }) 未传入 channelId
Then Widget 在初始化时控制台提示"channelId is required"
And 不渲染聊天组件
```

**场景 3：传入未配置的频道时无法使用**
```gherkin
Given 后端未配置频道 "channel-unknown"
When 调用 RWikiChat.init({ apiUrl: '...', channelId: 'channel-unknown' }) 后访客发送问题
Then 访客收到"频道不存在或不可用"的提示
```

---

## 故事 6：通过公开搜索能力直接检索知识库片段 [US-INTG-015]

**优先级**: P1

**【用户故事】**
**作为**：Website Integrator
**我希望**：在我的站点或应用中直接调用 RWiki 的公开搜索能力（指定频道与查询），获得按相关性排序的原始知识片段及来源信息
**从而**：无需接入聊天组件或 MCP 客户端，就能把知识库检索嵌入自己的页面与工具，且不为 LLM 生成付费

**【验收标准】**

**场景 1：检索成功**
```gherkin
Given 目标频道下存在已发布文档，且部分内容与查询相关
When 集成者调用公开搜索能力，携带该频道标识与查询，不携带任何凭据
Then 调用成功并返回该频道已发布文档中按相关性排序的知识片段列表
And 每条片段包含标题、章节、内容、来源链接、语言与相关性得分
And 得分含义为越高越相关
```

**场景 2：频道数据隔离**
```gherkin
Given 集成者可访问的两个频道下均存在与同一查询相关的内容
When 集成者分别以两个频道标识发起相同查询
Then 每次结果仅来自对应频道的已发布文档
```

**场景 3：无相关内容（零命中）**
```gherkin
Given 目标频道暂无已发布文档，或已发布内容经检索零命中
When 集成者调用公开搜索能力
Then 返回空结果列表，调用正常结束，不视为错误
```

**场景 4：频道无效**
```gherkin
Given 请求携带的频道标识未在服务配置中定义
When 集成者调用公开搜索能力
Then 请求被拒绝并返回明确提示，不返回任何知识库内容
```

## 故事 7：运行时切换 Widget 频道 [US-INTG-016]

**优先级**: P2

**【用户故事】**
**作为**：Website Integrator
**我希望**：不重建 Widget 就能调用 `RWikiChat.setChannel(channelId)` 切换目标频道
**从而**：多语言/多板块站点跟随上下文切换知识库频道（如文档站切到中文页时切到中文频道）

**【验收标准】**

**场景 1：切换即新会话**
```gherkin
Given Widget 已初始化
When 集成者调用 setChannel 传入另一频道
Then 在途回答被中止，当前对话与 sessionId 被清空，后续提问进入新频道的新会话
And Widget 不卸载重建，界面语言等其余配置保持
```

**场景 2：频道未变不清会话**
```gherkin
When 集成者传入与当前相同的频道集合（顺序不限）
Then 不发生任何切换动作，当前对话保留
```

**场景 3：非法参数**
```gherkin
When 传入空字符串/空数组/非法类型
Then 控制台报错，Widget 状态不变
```

## 故事 8：把检索词交给 Widget 直接提问 [US-INTG-017]

**优先级**: P1

**【用户故事】**
**作为**：Website Integrator
**我希望**：调用 `RWikiChat.ask(query)` 让宿主页面的检索入口（如站内搜索的「用 AI 回答」）直接发起一轮提问
**从而**：用户从搜索升级到 AI 问答无需手动复述问题

**【验收标准】**

**场景 1：提问自动发出**
```gherkin
Given Widget 已初始化
When 集成者调用 ask(query)
Then 聊天窗口打开，trim 后的 query 作为一轮用户提问自动发出
And 若已有回答在流式返回，按既有隐式中断语义收尾后发出新提问
```

**场景 2：空查询与未初始化**
```gherkin
When query 为空白或 Widget 未初始化
Then 不发起任何请求，未初始化时控制台报错
```
