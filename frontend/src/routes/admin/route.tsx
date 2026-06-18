import { createFileRoute, redirect } from '@tanstack/react-router'
import { isAuthenticated } from '@/lib/auth'
import { AdminLayout } from '@/components/admin/admin-layout'

// 集中守卫：自 index.tsx 迁移；子 route（index/low-recall）继承，无需各自重复。
// 401 由 api-client-setup.ts 全局拦截器处理，这里不做 useEffect 兜底鉴权。
export const Route = createFileRoute('/admin')({
  beforeLoad: () => {
    if (!isAuthenticated()) {
      throw redirect({ to: '/auth/login' })
    }
  },
  component: AdminLayout, // AdminLayout 内部渲染页头 + 导航 <Link> + <Outlet/>
})
