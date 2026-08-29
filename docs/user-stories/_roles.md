# 角色定义

| 角色 | 描述 | 典型场景 |
|------|------|----------|
| User | 个人知识库使用者。上传文档、提问、管理自己的知识库。 | 上传 xlsx、发起对话、查看文档列表 |
| Website Integrator | 网站集成者。通过嵌入 JS Widget 将 RWiki 聊天组件集成到第三方网站。 | 嵌入 Widget、定制外观、管理生命周期、配置推荐问题 |
| Admin | 管理员。通过 API Token 鉴权，管理知识库文档与系统配置。 | 知识库文档批量上下线、管理后台操作 |
| Agent Integrator | Agent 工具集成者。持 API Token 把 RWiki 知识库接入 MCP 客户端（Claude Code、Cursor 等 Agent 工具）。 | 在 MCP 客户端中添加 RWiki MCP 服务、调用知识库问答与检索工具 |
