/**
 * 首页路由
 *
 * 文件路径 src/routes/index.tsx → URL: /
 * 这是用户访问根路径时看到的页面。
 *
 * 修改指南：
 * - 修改首页内容 → 编辑下方 HomeRoute 组件
 * - 添加子路由 → 在 src/routes/ 下创建新文件
 */
import { createFileRoute } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: HomeRoute,
})

function HomeRoute() {
  return (
    <div className="flex items-center justify-center min-h-screen">
      <div className="text-center">
        <h1 className="text-4xl font-bold mb-4">
          Welcome to Rwiki
        </h1>
        <p className="text-muted-foreground">
          Project initialized. Start building!
        </p>
      </div>
    </div>
  )
}
