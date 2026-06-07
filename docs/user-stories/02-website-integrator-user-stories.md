# Website Integrator 用户故事

> 角色定义以 `docs/user-stories/_roles.md` 为准。

## 故事 1：嵌入聊天组件到第三方网站 [US-INTG-001]

**优先级**: P0

**【用户故事】**
**作为**：Website Integrator
**我希望**：通过引入一个 JS 文件并调用初始化函数，在我的网站中嵌入 RWiki 聊天组件
**从而**：我的网站访客可以直接在页面上向知识库提问，无需跳转

**【验收标准】**

**场景 1：成功嵌入并使用**
```gherkin
Given 网站已通过 <script> 标签引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: 'https://rwiki.example.com' })
Then 页面右下角出现浮动聊天按钮
And 点击按钮后弹出聊天对话框，可输入问题并获得流式回答
```

**场景 2：缺少必填配置**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init() 未传入 apiUrl
Then 控制台输出错误提示"apiUrl is required"
And 页面上不渲染任何 Widget 元素
```

**场景 3：后端不可达**
```gherkin
Given Widget 已初始化，但 apiUrl 指向不可达的服务
When 用户输入问题并发送
Then 聊天窗口显示"无法连接服务，请检查配置或稍后重试"
And 已发送的消息保留在聊天窗口中
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

**【验收标准】**

**场景 1：配置推荐问题后在 Widget 中展示**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', suggestedQuestions: { default: ['如何注册', '支持哪些支付方式'], en: ['How to register', 'Payment methods'] } })
Then Widget 打开后在空状态展示匹配访客浏览器语言的推荐问题按钮
And 访客点击按钮后直接发送问题并获得流式回答
```

**场景 2：配置为简单数组时正常展示**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...', suggestedQuestions: ['如何注册', '支持哪些支付方式', '如何联系客服'] })
Then Widget 打开后在空状态展示这 3 个推荐问题按钮
```

**场景 3：未配置推荐问题时 Widget 正常工作**
```gherkin
Given 网站已引入 rwiki-chat.js
When 调用 RWikiChat.init({ apiUrl: '...' }) 未传入 suggestedQuestions
Then Widget 正常渲染，空状态仅显示欢迎消息
And 访客可手动输入问题正常对话
```
