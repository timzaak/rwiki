import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { useChatModalStore } from '@/stores/chat-store'
import { WidgetApp } from '@/widget/widget-app'
import type { ValidatedWidgetConfig } from '@/widget/config'

/**
 * US-CORE-041 — Widget 不加载主站搜索框。
 *
 * 渲染真实 WidgetApp（不 mock ChatModal/FloatingButton），打开聊天弹窗
 * 证明组件树已完整挂载，再断言 search-input 不存在。有人把 SearchBox 接进
 * widget-app.tsx 或共享的 components/chat/* 时，此用例失败。
 */

const API_URL = 'http://localhost:3000'

function makeConfig(
  overrides: Partial<ValidatedWidgetConfig> = {},
): ValidatedWidgetConfig {
  return {
    apiUrl: API_URL,
    channelId: ['help-center'],
    primaryColor: '#3b82f6',
    position: 'right',
    displayMode: 'modal',
    locale: 'en',
    ...overrides,
  }
}

beforeAll(() => {
  // ChatPanel 挂载路径依赖 scrollIntoView，jsdom 未实现。
  Element.prototype.scrollIntoView = vi.fn()
})

beforeEach(() => {
  // useWidgetSuggestions 挂载即 fetch GET /api/chat/suggestions。
  server.use(
    http.get(`${API_URL}/api/chat/suggestions`, () =>
      HttpResponse.json({ questions: [] }),
    ),
  )
  useChatModalStore.setState({ isModalOpen: true })
})

afterEach(() => {
  useChatModalStore.setState({ isModalOpen: false })
  vi.clearAllMocks()
})

describe('Widget does not load the site search box', () => {
  it('renders the chat modal (tree mounted) with no search-input', async () => {
    render(<WidgetApp config={makeConfig()} />)

    // chat-modal 存在证明真实组件树已渲染，防止「渲染了空树」的空断言。
    expect(await screen.findByTestId('chat-modal')).toBeInTheDocument()
    expect(screen.queryByTestId('search-input')).toBeNull()
  })
})
