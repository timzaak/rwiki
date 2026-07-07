/**
 * 主站站点路由 `/s/$siteId`。
 *
 * 从路由参数读取 siteId → 校验站点存在（listSites）→ 渲染聊天 chrome 或
 * 未知/错误态。所有主站聊天、推荐问题、反馈请求的 siteId 均由
 * `SiteIdProvider` 向下透传（`use-chat-stream` / `use-feedback` 消费）。
 *
 * 未知站点态不渲染 FloatingButton / Provider / ModalMount，且不发任何
 * chat / suggestions / feedback 请求。
 */
import { lazy, Suspense, useEffect, useState } from 'react'
import { Outlet, createFileRoute } from '@tanstack/react-router'
import { LoaderCircleIcon, RefreshCwIcon } from 'lucide-react'

import { listSites, suggestions } from '@/lib/api-generated'
import { FloatingButton } from '@/components/chat/floating-button'
import { DefaultChatStreamProvider } from '@/components/chat/chat-stream-context'
import { SiteIdProvider } from '@/components/chat/site-id-context'
import { useChatModalStore } from '@/stores/chat-store'
import { Button } from '@/components/ui/button'

const ChatModal = lazy(() =>
  import('@/components/chat/chat-modal').then((module) => ({
    default: module.ChatModal,
  })),
)

// 缓存 key 为 `${siteId}:${locale}`，按站点隔离推荐问题。
const suggestionCache = new Map<string, string[]>()

export const Route = createFileRoute('/s/$siteId')({
  component: SiteRoute,
})

type SiteState = 'loading' | 'unknown' | 'error' | 'ready'

function SiteRoute() {
  // Route-bound useParams is strict-by-default in this TanStack version;
  // it returns the typed { siteId } for '/s/$siteId' with no extra opts.
  const { siteId } = Route.useParams()
  const [status, setStatus] = useState<SiteState>('loading')

  const validateSite = () => {
    setStatus('loading')
    let cancelled = false
    listSites()
      .then((result) => {
        if (cancelled) return
        const sites = result.data?.sites ?? []
        setStatus(sites.some((s) => s.id === siteId) ? 'ready' : 'unknown')
      })
      .catch(() => {
        if (!cancelled) setStatus('error')
      })
    return () => {
      cancelled = true
    }
  }

  useEffect(() => validateSite(), [siteId])

  if (status === 'loading') {
    return (
      <div
        data-testid="site-loading"
        className="flex min-h-screen items-center justify-center"
      >
        <LoaderCircleIcon className="size-6 animate-spin text-muted-foreground" />
      </div>
    )
  }

  if (status === 'error') {
    return (
      <div
        data-testid="site-error"
        className="flex min-h-screen flex-col items-center justify-center gap-4 px-6 text-center"
      >
        <p className="text-base text-muted-foreground">无法加载站点</p>
        <Button onClick={validateSite} variant="outline" size="sm">
          <RefreshCwIcon className="size-3.5" />
          重试
        </Button>
      </div>
    )
  }

  if (status === 'unknown') {
    return (
      <div
        data-testid="site-not-found"
        className="flex min-h-screen flex-col items-center justify-center px-6 text-center"
      >
        <p className="text-base text-muted-foreground">站点不存在或不可用</p>
      </div>
    )
  }

  return (
    <SiteIdProvider siteId={siteId}>
      <DefaultChatStreamProvider>
        <Outlet />
        <FloatingButton visible />
        <SiteChatModalMount siteId={siteId} />
      </DefaultChatStreamProvider>
    </SiteIdProvider>
  )
}

/**
 * 由 `/s/$siteId` 渲染的聊天弹窗挂载点（自 `__root.tsx` 迁入）。
 *
 * 仅在 `useChatModalStore.isModalOpen` 为真时挂载 `ChatModal`；
 * suggestions 调用携带路由 siteId，缓存 key 为 `${siteId}:${locale}`。
 */
function SiteChatModalMount({ siteId }: { siteId: string }) {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const locale = navigator.language
  const cacheKey = `${siteId}:${locale}`
  const [suggestedQuestions, setSuggestedQuestions] = useState<string[]>(
    () => suggestionCache.get(cacheKey) ?? [],
  )

  useEffect(() => {
    if (suggestionCache.has(cacheKey)) {
      setSuggestedQuestions(suggestionCache.get(cacheKey) ?? [])
      return
    }

    let cancelled = false
    suggestions({ query: { locale, siteId } })
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
  }, [cacheKey, locale, siteId])

  if (!isModalOpen) return null

  return (
    <Suspense fallback={null}>
      <ChatModal suggestedQuestions={suggestedQuestions} />
    </Suspense>
  )
}
