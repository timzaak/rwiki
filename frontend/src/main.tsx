/**
 * 应用入口文件
 *
 * 这是整个前端应用的起点。它做了以下事情：
 *
 * 1. 引入全局样式（styles.css，包含 Tailwind CSS）
 * 2. 创建 TanStack Router 路由实例（基于文件路由）
 * 3. 将路由挂载到 DOM
 *
 * 工作流程：
 * index.html → 加载此文件 → 创建 Router → 渲染到 #app 元素
 *
 * 扩展指南：
 * - 添加全局 Provider（如主题、国际化）→ 在 StrictMode 内添加
 * - 修改路由行为 → 编辑 src/routes/__root.tsx
 */
import './styles.css'
import { StrictMode } from 'react'
import { createRouter } from '@tanstack/react-router'
import { RouterProvider } from '@tanstack/react-router'
import ReactDOM from 'react-dom/client'
import { ThemeProvider } from 'next-themes'
import { routeTree } from './routeTree.gen'

// 创建路由器实例
// routeTree 是由 TanStack Router 插件根据 src/routes/ 目录自动生成的
// 每个文件对应一个路由，文件路径决定 URL 路径
export const router = createRouter({
  routeTree,
})

// TypeScript 类型声明 — 让路由器知道可用的路由类型
// 这提供了完整的类型安全和自动补全
declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}

declare global {
  interface Window {
    router: typeof router
  }
}

// 开发模式下将路由实例暴露到 window，方便调试
if (import.meta.env.DEV) {
  window.router = router
}

// 渲染应用
// StrictMode 会在开发模式下触发额外的渲染来检测副作用
const rootElement = document.getElementById('app')!

ReactDOM.createRoot(rootElement).render(
  <StrictMode>
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
      <RouterProvider router={router} />
    </ThemeProvider>
  </StrictMode>
)
