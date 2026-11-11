/**
 * 主站频道路由 `/c/$channelId`。
 *
 * 从路由参数读取 channelId → 校验频道存在（listChannels）→ 渲染聊天 chrome 或
 * 未知/错误态。所有主站聊天、推荐问题、反馈请求的 channelId 均由
 * `ChannelIdProvider` 向下透传（`use-chat-stream` / `use-feedback` 消费）。
 *
 * 搜索框（SearchBox）只在 ready 分支渲染：频道校验中或频道不存在时不出现，
 * 且不发任何搜索请求。
 *
 * 未知频道态不渲染 FloatingButton / Provider / ModalMount，且不发任何
 * chat / suggestions / feedback 请求。
 */
import { lazy, Suspense, useEffect, useState } from 'react'
import { Outlet, createFileRoute } from '@tanstack/react-router'
import { LoaderCircleIcon, RefreshCwIcon } from 'lucide-react'

import { listChannels, suggestions } from '@/lib/api-generated'
import { FloatingButton } from '@/components/chat/floating-button'
import {
  DefaultChatStreamProvider,
  useChatStreamContext,
} from '@/components/chat/chat-stream-context'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'
import { SearchBox } from '@/components/search/search-box'
import { useChatModalStore } from '@/stores/chat-store'
import { rebindWorkspaceChannel } from '@/stores/chat-history-store'
import { channelKeyOf } from '@/lib/channel-key'
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
    <ChannelIdProvider channelId={[channelId]}>
      <DefaultChatStreamProvider>
        <main className="mx-auto w-full max-w-2xl px-4 pt-16">
          <ChannelSearchBox key={channelId} channelId={channelId} />
        </main>
        <Outlet />
        <FloatingButton visible />
        <ChannelChatBinding channelId={channelId} />
        <ChannelChatModalMount channelId={channelId} />
      </DefaultChatStreamProvider>
    </ChannelIdProvider>
  )
}

/**
 * 聊天工作区频道绑定：频道变化时执行换频道协议（归档旧会话 → 清空 → 绑定
 * 新频道，见 chat-history-store 的 rebindWorkspaceChannel），保证历史按频道
 * 隔离且跨频道遗留数据不丢失。
 */
function ChannelChatBinding({ channelId }: { channelId: string }) {
  useEffect(() => {
    rebindWorkspaceChannel(channelKeyOf([channelId]))
  }, [channelId])
  return null
}

/**
 * 频道搜索框挂载点：在 ChatStream Provider 内注入「用 AI 回答」——打开聊天
 * 弹窗并把检索词作为一轮新提问发出（US-CORE-044）。与在聊天框输入等价：
 * 续接当前会话；有回答在途时由 sendMessage 中断并保留。
 */
function ChannelSearchBox({ channelId }: { channelId: string }) {
  const { sendMessage } = useChatStreamContext()
  const openModal = useChatModalStore((s) => s.openModal)

  return (
    <SearchBox
      channelId={channelId}
      onAskAi={(query) => {
        openModal()
        sendMessage(query)
      }}
    />
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
    suggestions({ query: { locale, channelId: [channelId] } })
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
