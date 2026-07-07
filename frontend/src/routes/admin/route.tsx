import { createFileRoute, redirect } from '@tanstack/react-router'
import { isAuthenticated } from '@/lib/auth'
import { AdminLayout } from '@/components/admin/admin-layout'
import { AdminSiteProvider } from '@/lib/admin-site-context'

// 集中守卫：自 index.tsx 迁移；子 route（index/low-recall）继承，无需各自重复。
// 401 由 api-client-setup.ts 全局拦截器处理，这里不做 useEffect 兜底鉴权。
export const Route = createFileRoute('/admin')({
  beforeLoad: () => {
    if (!isAuthenticated()) {
      throw redirect({ to: '/auth/login' })
    }
  },
  // 全局站点上下文：包裹 AdminLayout，使所有 admin 子页共享同一 siteId（顶部
  // `admin-site-select` 切换后各列表/操作按新 siteId 重取）。守卫仍在 beforeLoad。
  component: () => (
    <AdminSiteProvider>
      <AdminLayout />
    </AdminSiteProvider>
  ),
})
