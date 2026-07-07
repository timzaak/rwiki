import { Link, Outlet } from '@tanstack/react-router'
import { useAdminSite } from '@/lib/admin-site-context'
import { Button } from '@/components/ui/button'

/**
 * Admin 后台共享布局。
 *
 * 由 `routes/admin/route.tsx`（`createFileRoute('/admin')`）渲染，外层已包裹
 * `AdminSiteProvider`。本布局在顶部导航上方渲染全局站点选择区（原生
 * `<select data-testid="admin-site-select">`），所有 admin 子页共享该 siteId，
 * 切换后各列表/操作按新 siteId 重取。
 *
 * 三个状态：
 *   - 加载中 → selector 旁「Loading…」。
 *   - 失败 → 错误提示 + 手动重试按钮（触发 `retry`）。
 *   - 空站点（异常）→ 禁用 selector + 「No sites configured, contact admin」。
 *
 * 子 route（`/admin/`、`/admin/low-recall`）内容渲染于 `<Outlet/>`，各子页自带
 * 页面级容器与 testid（`admin-page` / `low-recall-page`）。
 *
 * 守卫（未登录 → `/auth/login`）集中在父 route 的 `beforeLoad`，本组件不重复。
 */
export function AdminLayout() {
  const { siteId, setSiteId, sites, loading, error, retry } = useAdminSite()

  return (
    <>
      <div
        data-testid="admin-site-bar"
        className="mx-auto flex w-full max-w-5xl flex-wrap items-center gap-2 px-6 pt-6 text-sm"
      >
        <span className="font-medium text-muted-foreground">Site</span>
        <select
          data-testid="admin-site-select"
          value={siteId ?? ''}
          onChange={(e) => setSiteId(e.target.value)}
          disabled={loading || sites.length === 0}
          className="h-8 rounded-lg border border-border/60 bg-card px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        >
          {/* 占位项：仅在未选定且非空列表时显示（空列表时 select 整体 disabled）。 */}
          {siteId === null && sites.length > 0 ? (
            <option value="" disabled>
              Select site…
            </option>
          ) : null}
          {sites.map((site) => (
            <option key={site.id} value={site.id}>
              {site.name}
            </option>
          ))}
        </select>

        {loading ? (
          <span data-testid="admin-site-loading" className="text-xs text-muted-foreground">
            Loading…
          </span>
        ) : null}

        {error ? (
          <span className="flex items-center gap-2">
            <span data-testid="admin-site-error" className="text-xs text-destructive">
              {error}
            </span>
            <Button
              type="button"
              variant="outline"
              size="xs"
              data-testid="admin-site-retry"
              onClick={retry}
            >
              Retry
            </Button>
          </span>
        ) : null}

        {!loading && !error && sites.length === 0 ? (
          <span
            data-testid="admin-site-empty"
            className="text-xs text-muted-foreground"
          >
            No sites configured, contact admin
          </span>
        ) : null}
      </div>

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
