/**
 * 根路由 — 仅承载 `<Outlet/>`。
 *
 * 主站聊天 chrome（FloatingButton / DefaultChatStreamProvider /
 * LazyChatModalMount / suggestions）已迁入 `/s/$siteId` 路由（见
 * `routes/s/$siteId.tsx`），根路径不再承载无站点聊天。
 */
import { createRootRoute, Outlet } from '@tanstack/react-router'

export const Route = createRootRoute({
  component: RootComponent,
})

function RootComponent() {
  return <Outlet />
}
