/**
 * OpenAPI TypeScript 客户端生成配置
 *
 * 这个文件配置 @hey-api/openapi-ts 工具，它会：
 * 1. 读取后端导出的 OpenAPI 规范（api.json）
 * 2. 生成类型安全的 TypeScript API 客户端代码
 * 3. 输出到 src/lib/api-generated/ 目录
 *
 * 使用方式：
 * - npm run generate-api — 手动触发生成
 * - npm run dev / npm run build — 自动在构建前生成
 *
 * 修改指南：
 * - 后端新增接口后，重新运行 generate-api 即可自动更新客户端
 * - 需要自定义请求处理 → 修改 services 配置
 * - 使用其他 HTTP 客户端 → 修改 client 字段（如改为 'fetch'）
 */
import { defineConfig } from '@hey-api/openapi-ts'

export default defineConfig({
  input: './api.json',
  output: {
    path: './src/lib/api-generated',
  },
  services: {
    asClass: false,
    name: '{{name}}',
    include: 'responses|requests|all',
    operationId: true,
    response: 'body',
  },
  client: 'axios',
})
