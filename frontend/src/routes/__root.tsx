/**
 * 根路由 — 全局布局：聊天流 Provider + 浮动按钮 + 按需加载的聊天弹窗
 */
import { lazy, Suspense, useState, useEffect } from 'react'
import { createRootRoute, Outlet } from '@tanstack/react-router'
import { FloatingButton } from '@/components/chat/floating-button'
import { DefaultChatStreamProvider } from '@/components/chat/chat-stream-context'
import { useChatModalStore } from '@/stores/chat-store'
import { suggestions } from '@/lib/api-generated/sdk.gen'

const ChatModal = lazy(() =>
  import('@/components/chat/chat-modal').then((module) => ({
    default: module.ChatModal,
  })),
)

const suggestionCache = new Map<string, string[]>()

export const Route = createRootRoute({
  component: RootComponent,
})

function RootComponent() {
  return (
    <>
      <DefaultChatStreamProvider>
        <Outlet />
        <FloatingButton visible />
        <LazyChatModalMount />
      </DefaultChatStreamProvider>
    </>
  )
}

function LazyChatModalMount() {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const [suggestedQuestions, setSuggestedQuestions] = useState<string[]>(() => {
    const cached = suggestionCache.get(navigator.language)
    return cached ?? []
  })

  useEffect(() => {
    const locale = navigator.language
    if (suggestionCache.has(locale)) return

    let cancelled = false
    suggestions({ query: { locale } })
      .then((result) => {
        if (!cancelled && result.data) {
          suggestionCache.set(locale, result.data.questions)
          setSuggestedQuestions(result.data.questions)
        }
      })
      .catch(() => {})
    return () => { cancelled = true }
  }, [])

  if (!isModalOpen) return null

  return (
    <Suspense fallback={null}>
      <ChatModal suggestedQuestions={suggestedQuestions} />
    </Suspense>
  )
}
