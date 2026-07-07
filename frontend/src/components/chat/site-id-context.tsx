import { createContext, useContext } from 'react'

/**
 * 主站 siteId 单一来源。
 *
 * 仅由 `/s/$siteId` 路由的 `SiteIdProvider` 注入；`use-chat-stream` 与
 * `use-feedback` 通过 `useSiteId()` 消费，避免跨组件 prop drilling。
 * Widget 不经此 context（Widget 用 `config.siteId` + 自带 submitFn）。
 */
const SiteIdContext = createContext<string | null>(null)

export function SiteIdProvider({
  siteId,
  children,
}: {
  siteId: string
  children: React.ReactNode
}) {
  return (
    <SiteIdContext.Provider value={siteId}>{children}</SiteIdContext.Provider>
  )
}

/**
 * 读取主站 siteId。必须在 `SiteIdProvider` 内调用，否则抛错——
 * 这强制 `/s/$siteId` 路由在渲染聊天 chrome 前先完成站点校验并包裹 Provider。
 */
export function useSiteId(): string {
  const siteId = useContext(SiteIdContext)
  if (siteId === null) {
    throw new Error('useSiteId must be used within a SiteIdProvider')
  }
  return siteId
}
