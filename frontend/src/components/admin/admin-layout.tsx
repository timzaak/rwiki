import { Link, Outlet } from '@tanstack/react-router'

/**
 * Admin 后台共享布局。
 *
 * 由 `routes/admin/route.tsx`（`createFileRoute('/admin')`）渲染，
 * 只提供页内导航（Document Management / Low-Recall Records）。
 * 子 route（`/admin/`、`/admin/low-recall`）内容渲染于 `<Outlet/>`，
 * 各子页自带页面级容器与 testid（`admin-page` / `low-recall-page`），
 * 因此本布局只承载导航容器 testid `admin-nav`，不再叠加页面级容器/标题。
 *
 * 守卫（未登录 → `/auth/login`）集中在父 route 的 `beforeLoad`，
 * 本组件不重复、不靠 useEffect 兜底（参照 TanStack layout route 惯例）。
 */
export function AdminLayout() {
  return (
    <>
      <nav
        data-testid="admin-nav"
        className="mx-auto flex w-full max-w-5xl items-center gap-4 px-6 pt-8 text-sm"
      >
        <Link
          to="/admin"
          activeProps={{
            className: 'font-semibold text-foreground',
          }}
          inactiveProps={{
            className: 'text-muted-foreground',
          }}
        >
          Document Management
        </Link>
        <Link
          to="/admin/low-recall"
          activeProps={{
            className: 'font-semibold text-foreground',
          }}
          inactiveProps={{
            className: 'text-muted-foreground',
          }}
        >
          Low-Recall Records
        </Link>
      </nav>

      <Outlet />
    </>
  )
}
