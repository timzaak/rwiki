/**
 * 根路由文件
 *
 * TanStack Router 使用文件系统路由：
 * - src/routes/__root.tsx → 所有路由的父级布局
 * - src/routes/index.tsx → 首页 (/)
 * - src/routes/about.tsx → /about
 * - src/routes/auth/login.tsx → /auth/login
 * - src/routes/manage/dashboard.tsx → /manage/dashboard
 *
 * 文件名以 __ 开头的表示布局路由（不会产生 URL 段）
 * Outlet 组件是子路由渲染的位置
 *
 * 这个文件定义了：
 * 1. 聊天流 Provider
 * 2. 浮动按钮和按需加载的聊天弹窗
 *
 * 扩展指南：
 * - 添加全局导航栏 → 在 Outlet 上方添加 <nav> 组件
 * - 添加全局侧边栏 → 在 Outlet 旁添加 sidebar 组件
 * - 添加认证守卫 → 在 beforeLoad 或 loader 中检查认证
 */
import { lazy, Suspense, useState, useEffect } from 'react'
import { createRootRoute, Outlet, useLocation } from '@tanstack/react-router'
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

// 创建根路由
export const Route = createRootRoute({
  // beforeLoad — 在路由加载前执行（适合做认证检查）
  // loader — 在路由加载前加载数据
  component: RootComponent,
})

function RootComponent() {
  const location = useLocation()
  const isChatRoute = location.pathname === '/chat'

  return (
    <>
      <DefaultChatStreamProvider>
        <Outlet />
        <FloatingButton visible={!isChatRoute} />
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
