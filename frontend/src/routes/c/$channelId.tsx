/**
 * 主站频道路由 `/c/$channelId`。
 *
 * 从路由参数读取 channelId → 校验频道存在（listChannels）→ 渲染聊天 chrome 或
 * 未知/错误态。所有主站聊天、推荐问题、反馈请求的 channelId 均由
 * `ChannelIdProvider` 向下透传（`use-chat-stream` / `use-feedback` 消费）。
 *
 * 未知频道态不渲染 FloatingButton / Provider / ModalMount，且不发任何
 * chat / suggestions / feedback 请求。
 */
import { lazy, Suspense, useEffect, useState } from 'react'
import { Outlet, createFileRoute } from '@tanstack/react-router'
import { LoaderCircleIcon, RefreshCwIcon } from 'lucide-react'

import { listChannels, suggestions } from '@/lib/api-generated'
import { FloatingButton } from '@/components/chat/floating-button'
import { DefaultChatStreamProvider } from '@/components/chat/chat-stream-context'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'
import { useChatModalStore } from '@/stores/chat-store'
import { Button } from '@/components/ui/button'

const ChatModal = lazy(() =>
  import('@/components/chat/chat-modal').then((module) => ({
    default: module.ChatModal,
  })),
)

// 缓存 key 为 `${channelId}:${locale}`，按频道隔离推荐问题。
const suggestionCache = new Map<string, string[]>()

export const Route = createFileRoute('/c/$channelId')({
  component: ChannelRoute,
})

type ChannelState = 'loading' | 'unknown' | 'error' | 'ready'

function ChannelRoute() {
  // Route-bound useParams is strict-by-default in this TanStack version;
  // it returns the typed { channelId } for '/c/$channelId' with no extra opts.
  const { channelId } = Route.useParams()
  const [status, setStatus] = useState<ChannelState>('loading')

  const validateChannel = () => {
    setStatus('loading')
    let cancelled = false
    listChannels()
      .then((result) => {
        if (cancelled) return
        const channels = result.data?.channels ?? []
        setStatus(channels.some((c) => c.id === channelId) ? 'ready' : 'unknown')
      })
      .catch(() => {
        if (!cancelled) setStatus('error')
      })
    return () => {
      cancelled = true
    }
  }

  useEffect(() => validateChannel(), [channelId])

  if (status === 'loading') {
    return (
      <div
        data-testid="channel-loading"
        className="flex min-h-screen items-center justify-center"
      >
        <LoaderCircleIcon className="size-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (status === 'error') {
    return (
      <div
        data-testid="channel-error"
        className="flex min-h-screen flex-col items-center justify-center gap-4 px-6 text-center"
      >
        <p className="text-base text-muted-foreground">无法加载频道</p>
        <Button onClick={validateChannel} variant="outline" size="sm">
          <RefreshCwIcon className="size-3.5" />
          重试
        </Button>
      </div>
    )
  }

  if (status === 'unknown') {
    return (
      <div
        data-testid="channel-not-found"
        className="flex min-h-screen flex-col items-center justify-center px-6 text-center"
      >
        <p className="text-base text-muted-foreground">频道不存在或不可用</p>
      </div>
    )
  }

  return (
    <ChannelIdProvider channelId={channelId}>
      <DefaultChatStreamProvider>
        <Outlet />
        <FloatingButton visible />
        <ChannelChatModalMount channelId={channelId} />
      </DefaultChatStreamProvider>
    </ChannelIdProvider>
  )
}

/**
 * 由 `/c/$channelId` 渲染的聊天弹窗挂载点（自 `__root.tsx` 迁入）。
 *
 * 仅在 `useChatModalStore.isModalOpen` 为真时挂载 `ChatModal`；
 * suggestions 调用携带路由 channelId，缓存 key 为 `${channelId}:${locale}`。
 */
function ChannelChatModalMount({ channelId }: { channelId: string }) {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const locale = navigator.language
  const cacheKey = `${channelId}:${locale}`
  const [suggestedQuestions, setSuggestedQuestions] = useState<string[]>(
    () => suggestionCache.get(cacheKey) ?? [],
  )

  useEffect(() => {
    if (suggestionCache.has(cacheKey)) {
      setSuggestedQuestions(suggestionCache.get(cacheKey) ?? [])
      return
    }

    let cancelled = false
    suggestions({ query: { locale, channelId } })
      .then((result) => {
        if (!cancelled && result.data) {
          suggestionCache.set(cacheKey, result.data.questions)
          setSuggestedQuestions(result.data.questions)
        }
      })
      .catch(() => {})
    return () => {
      cancelled = true
    }
  }, [cacheKey, locale, channelId])

  if (!isModalOpen) return null

  return (
    <Suspense fallback={null}>
      <ChatModal suggestedQuestions={suggestedQuestions} />
    </Suspense>
  )
}
