import { createContext, useContext } from 'react'

/**
 * 主站 channelId 单一来源。
 *
 * 仅由 `/c/$channelId` 路由的 `ChannelIdProvider` 注入；`use-chat-stream` 与
 * `use-feedback` 通过 `useChannelId()` 消费，避免跨组件 prop drilling。
 * Widget 不经此 context（Widget 用 `config.channelId` + 自带 submitFn）。
 */
const ChannelIdContext = createContext<string | null>(null)

export function ChannelIdProvider({
  channelId,
  children,
}: {
  channelId: string
  children: React.ReactNode
}) {
  return (
    <ChannelIdContext.Provider value={channelId}>{children}</ChannelIdContext.Provider>
  )
}

/**
 * 读取主站 channelId。必须在 `ChannelIdProvider` 内调用，否则抛错——
 * 这强制 `/c/$channelId` 路由在渲染聊天 chrome 前先完成频道校验并包裹 Provider。
 */
export function useChannelId(): string {
  const channelId = useContext(ChannelIdContext)
  if (channelId === null) {
    throw new Error('useChannelId must be used within a ChannelIdProvider')
  }
  return channelId
}
