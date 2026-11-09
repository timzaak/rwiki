import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { act, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { useChatModalStore, useChatStore } from '@/stores/chat-store'

/**
 * US-INTG-016 / US-INTG-017 — 真实组件树下的 RWikiChat.ask / setChannel。
 *
 * main.test.tsx 用 mock 的 createRoot 校验全局 API 的调用契约；这里挂载真实
 * WidgetApp，断言用户可感知的结果：ask 的问题真的作为一轮聊天发往后端、只发一次、
 * 且发往「当前」频道——包括宿主在同一 tick 内先 setChannel 再 ask 的情况。
 */

vi.mock('../styles.css?inline', () => ({ default: '' }))
vi.mock('highlight.js/styles/github.css?inline', () => ({ default: '' }))
vi.mock('../inject-styles', () => ({ injectStyles: vi.fn() }))

// Side-effect import: assigns window.RWikiChat (real createRoot + WidgetApp).
await import('../main')

const API_URL = 'http://localhost:3000'

type ChatBody = { message: string; channelId: string[]; sessionId: string | null }

function api() {
  return (window as any).RWikiChat as {
    init: (config: unknown) => void
    destroy: () => void
    ask: (query: string) => void
    setChannel: (channelId: string | string[]) => void
  }
}

let chatBodies: ChatBody[]

beforeEach(() => {
  chatBodies = []
  server.use(
    http.get(`${API_URL}/api/chat/suggestions`, () => HttpResponse.json({ questions: [] })),
    http.post(`${API_URL}/api/chat`, async ({ request }) => {
      chatBodies.push((await request.json()) as ChatBody)
      // An immediately-closed SSE stream: the round finishes normally.
      const stream = new ReadableStream({ start: (controller) => controller.close() })
      return new Response(stream, { headers: { 'Content-Type': 'text/event-stream' } })
    }),
  )
})

afterEach(async () => {
  await act(async () => api().destroy())
  useChatModalStore.setState({ isModalOpen: false })
})

describe('RWikiChat.ask / setChannel on a mounted widget', () => {
  it('ask() sends the query once as a chat turn to the current channel', async () => {
    await act(async () => api().init({ apiUrl: API_URL, channelId: 'help_center' }))

    await act(async () => api().ask('  what is rwiki  '))

    await waitFor(() => expect(chatBodies).toHaveLength(1))
    expect(chatBodies[0]).toMatchObject({ message: 'what is rwiki', channelId: ['help_center'] })
    expect(useChatModalStore.getState().isModalOpen).toBe(true)
    expect(useChatStore.getState().messages[0]).toMatchObject({
      role: 'user',
      content: 'what is rwiki',
    })
    // Consumed exactly once: later renders must not re-send the question.
    await act(async () => {})
    expect(chatBodies).toHaveLength(1)
    expect(useChatStore.getState().pendingQuestion).toBeNull()
  })

  it('ask() right after init() is delivered once the widget has mounted', async () => {
    // Hosts typically call init() in the script onload and may ask() at once,
    // before React has rendered anything.
    await act(async () => {
      api().init({ apiUrl: API_URL, channelId: 'help_center' })
      api().ask('hello')
    })

    await waitFor(() => expect(chatBodies).toHaveLength(1))
    expect(chatBodies[0]).toMatchObject({ message: 'hello', channelId: ['help_center'] })
  })

  it('ask() right after setChannel() goes to the new channel in a new session', async () => {
    await act(async () => api().init({ apiUrl: API_URL, channelId: 'help_center' }))
    useChatStore.setState({ sessionId: 'old-session' })

    // Same tick, like the docs site switching language then handing over a query.
    await act(async () => {
      api().setChannel('help_center_zh')
      api().ask('如何配置重排序')
    })

    await waitFor(() => expect(chatBodies).toHaveLength(1))
    expect(chatBodies[0]).toMatchObject({
      message: '如何配置重排序',
      channelId: ['help_center_zh'],
      sessionId: null,
    })
  })
})
