# Admin 用户故事

> 角色定义以 `docs/user-stories/_roles.md` 为准。

## 故事 1：上传 OpenAPI 规范 [US-ADMIN-001]

**优先级**: P0

**【用户故事】**
**作为**：Admin
**我希望**：上传 OpenAPI JSON 文件作为系统当前生效的 API 规范
**从而**：普通用户可以基于该规范通过自然语言编排 API 调用

**【验收标准】**

**场景 1：上传合法且注释充分的 OpenAPI JSON**
```gherkin
Given 系统当前无生效的 OpenAPI 规范
When 管理员上传一份 OpenAPI 3.1.0 JSON 文件，结构合法且接口注释充分
Then 系统返回上传成功，包含规范名称、版本号、接口数量和校验摘要
And 该规范成为当前唯一生效版本
And 普通用户可以使用 API 编排功能
```

**场景 2：上传格式错误的 JSON**
```gherkin
Given 管理员准备上传 OpenAPI 规范
When 上传的文件不是合法 JSON
Then 系统拒绝该文件，返回错误提示"JSON 格式无效"
And 系统不替换当前生效规范（如果有）
```

**场景 3：上传结构不合规的 OpenAPI**
```gherkin
Given 管理员准备上传 OpenAPI 规范
When 上传的 JSON 结构不符合 OpenAPI 3.1.0 要求（例如缺少 paths、缺少 operationId）
Then 系统拒绝该文件，返回具体问题列表，包含每个问题的位置和说明
And 系统不替换当前生效规范
```

**场景 4：上传注释不充分的 OpenAPI**
```gherkin
Given 管理员上传的 OpenAPI JSON 结构合法
When 系统评估后认为接口注释不足以支持 AI 正确编排（如必填参数缺少业务说明）
Then 系统拒绝该文件，返回注释问题列表
And 系统不替换当前生效规范
```

**场景 5：上传新规范替换旧规范**
```gherkin
Given 系统当前已有一份生效的 OpenAPI 规范
When 管理员上传一份新的合法 OpenAPI JSON
Then 新规范成为唯一生效版本
And 旧规范不再对用户可见
```

**场景 6：上传带有警告但无阻断问题的规范**
```gherkin
Given 管理员上传的 OpenAPI JSON 结构合法且注释评估通过
When 校验过程中发现非关键问题（如部分接口缺少描述）
Then 系统允许生效，同时返回警告列表供管理员参考
And 该规范成为当前唯一生效版本
```

---

## 故事 2：查看当前生效的 OpenAPI 规范 [US-ADMIN-002]

**优先级**: P1

**【用户故事】**
**作为**：Admin
**我希望**：查看系统当前生效的 OpenAPI 规范的元数据信息
**从而**：确认当前规范状态和接口覆盖情况

**【验收标准】**

**场景 1：有生效规范时查看**
```gherkin
Given 系统当前有一份生效的 OpenAPI 规范
When 管理员查看规范信息
Then 系统显示规范名称、版本号、上传时间、接口数量和校验摘要
```

**场景 2：无生效规范时查看**
```gherkin
Given 系统当前没有生效的 OpenAPI 规范
When 管理员查看规范信息
Then 系统显示"暂无生效的 API 规范"
And 提示管理员上传 OpenAPI JSON
```

---

## 故事 3：清空当前生效的 OpenAPI 规范 [US-ADMIN-003]

**优先级**: P2

**【用户故事】**
**作为**：Admin
**我希望**：清空系统当前生效的 OpenAPI 规范
**从而**：在不再需要 API 编排功能时停止该能力

**【验收标准】**

**场景 1：清空生效规范**
```gherkin
Given 系统当前有一份生效的 OpenAPI 规范
When 管理员确认清空操作
Then 系统移除当前生效规范
And 普通用户的 API 编排功能不可用
And 普通用户在尝试 API 编排时收到"暂无可用的 API 规范"提示
```

**场景 2：无生效规范时清空**
```gherkin
Given 系统当前没有生效的 OpenAPI 规范
When 管理员尝试清空操作
Then 系统提示"当前无生效规范，无需清空"
```
