import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { createSseResponse } from '@/test/helpers/sse'
import { useChatStream } from '@/hooks/use-chat-stream'
import { useChatStore } from '@/stores/chat-store'
import { client } from '@/lib/api-generated/client.gen'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'

/**
 * FE-D02 regression: `useChatStream` now reads `useChannelId()` at the top, so
 * every hook render must be wrapped in `ChannelIdProvider` or it throws. The
 * wrappers below inject the main-site channelId that flows into the chat request
 * body (`channelId` alongside `message`/`sessionId`).
 */
function makeWrapper(channelId = 'channel-a') {
  return ({ children }: { children: React.ReactNode }) => (
    <ChannelIdProvider channelId={channelId}>{children}</ChannelIdProvider>
  )
}

function resetStore() {
  useChatStore.setState({
    messages: [],
    sessionId: null,
    isLoading: false,
    error: null,
  })
}

function getLastAssistantMessage() {
  const { messages } = useChatStore.getState()
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'assistant') return messages[i]
  }
  return undefined
}

// The generated SSE client uses new Request(url, init) internally, which
// requires an absolute URL in the MSW/Node.js test environment.  Setting
// baseUrl on the generated client ensures buildUrl produces an absolute URL.
beforeEach(() => {
  client.setConfig({ baseUrl: 'http://localhost:3000' })
})

afterEach(() => {
  client.setConfig({ baseUrl: '' })
})

describe('useChatStream sending a message', () => {
  beforeEach(() => {
    resetStore()
  })

  it('sends message and receives session + chunk + done events', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-abc' } },
          { event: 'chunk', data: { content: 'Hello from AI' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('What is the sales data?')

    const state = useChatStore.getState()

    // User message was added
    expect(state.messages.some((m) => m.role === 'user' && m.content === 'What is the sales data?')).toBe(true)

    // Assistant message placeholder was added and updated
    const assistant = getLastAssistantMessage()
    expect(assistant).toBeDefined()
    expect(assistant!.content).toBe('Hello from AI')

    // sessionId was set from the session event
    expect(state.sessionId).toBe('sess-abc')

    // isLoading and isStreaming are false after done
    expect(state.isLoading).toBe(false)
    expect(assistant!.isStreaming).toBe(false)
  })

  it('accumulates multiple chunk events into the assistant message', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-multi' } },
          { event: 'chunk', data: { content: 'Hello' } },
          { event: 'chunk', data: { content: ' world' } },
          { event: 'chunk', data: { content: '!' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const assistant = getLastAssistantMessage()
    expect(assistant!.content).toBe('Hello world!')
    expect(useChatStore.getState().isLoading).toBe(false)
  })

  it('preserves displayed content and sets error on SSE error event', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-err' } },
          { event: 'chunk', data: { content: 'Partial' } },
          { event: 'chunk', data: { content: ' content' } },
          { event: 'error', data: { message: 'Internal server error' } },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const state = useChatStore.getState()

    // Assistant message still has accumulated content
    const assistant = getLastAssistantMessage()
    expect(assistant!.content).toBe('Partial content')

    // Error is set
    expect(state.error).toBe('Internal server error')
    expect(state.isLoading).toBe(false)
  })

  it('preserves content and finishes streaming on network failure', async () => {
    server.use(
      http.post('/api/chat', () => {
        return HttpResponse.error()
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const state = useChatStore.getState()

    // The SSE client swallows the network error (sseMaxRetryAttempts=0)
    // so the hook finishes streaming without setting an error
    expect(state.isLoading).toBe(false)
  })

  it('stopStreaming aborts the stream', async () => {
    const encoder = new TextEncoder()

    // A stream that emits a session event then stays open until aborted.
    const delayedStream = new ReadableStream({
      start(controller) {
        controller.enqueue(
          encoder.encode(
            'event: session\ndata: {"sessionId":"sess-abort"}\n\n',
          ),
        )
        // Do NOT enqueue done — the stream stays open until stopStreaming aborts
      },
    })

    server.use(
      http.post('/api/chat', () => {
        return new Response(delayedStream, {
          headers: {
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
          },
        })
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    // Start sending, then immediately abort
    const sendPromise = result.current.sendMessage('test')
    result.current.stopStreaming()

    await sendPromise

    await waitFor(() => {
      expect(useChatStore.getState().isLoading).toBe(false)
    })
  })
})

describe('useChatStream request body validation', () => {
  beforeEach(() => {
    resetStore()
  })

  it('sends correct ChatRequest body to the API', async () => {
    let capturedBody: unknown = null

    server.use(
      http.post('/api/chat', async ({ request }) => {
        capturedBody = await request.json()
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-body' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test question')

    expect(capturedBody).toEqual({
      message: 'test question',
      sessionId: null,
      // FE-T02 load-bearing contract: channelId from ChannelIdProvider is transmitted
      // in the chat request body alongside message/sessionId.
      channelId: 'channel-a',
    })
  })

  it('changes body.channelId when the provider channelId changes on rerender', async () => {
    // Guards against channelId being hoisted out of the sendMessage dependency
    // array: if the hook closed over a stale channelId, the rerendered send would
    // still carry the old value. The wrapper reads a mutable ref so the same
    // wrapper instance can supply a different channelId after `rerender`.
    let capturedBody: unknown = null
    let currentChannelId = 'channel-a'

    server.use(
      http.post('/api/chat', async ({ request }) => {
        capturedBody = await request.json()
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-rerender' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <ChannelIdProvider channelId={currentChannelId}>{children}</ChannelIdProvider>
    )

    const { result, rerender } = renderHook(() => useChatStream(), { wrapper })

    // Flip the provider channelId, then re-render so the hook re-reads context.
    currentChannelId = 'channel-b'
    rerender()

    await result.current.sendMessage('after rerender')

    const body = capturedBody as { channelId?: string } | null
    expect(body).not.toBeNull()
    expect(body!.channelId).toBe('channel-b')
  })

  it('sends sessionId in request when store has one', async () => {
    let capturedBody: unknown = null

    // Pre-set a sessionId in the store
    useChatStore.setState({ sessionId: 'existing-session-123' })

    server.use(
      http.post('/api/chat', async ({ request }) => {
        capturedBody = await request.json()
        return createSseResponse([
          { event: 'session', data: { sessionId: 'new-session-456' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('follow-up question')

    expect(capturedBody).toEqual({
      message: 'follow-up question',
      sessionId: 'existing-session-123',
      // FE-T02: channelId is transmitted on follow-up messages too.
      channelId: 'channel-a',
    })
  })
})

describe('useChatStream post-answer suggestions', () => {
  beforeEach(() => {
    resetStore()
  })

  it('parses suggestions event and writes to last assistant suggestedQuestions, then done still finishes streaming', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-sugg' } },
          { event: 'chunk', data: { content: 'Final answer' } },
          {
            event: 'suggestions',
            data: { suggestions: ['What is X?', 'How does Y work?'] },
          },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const state = useChatStore.getState()
    const assistant = getLastAssistantMessage()

    // suggestions event was parsed and written to the last assistant message
    expect(assistant).toBeDefined()
    expect(assistant!.suggestedQuestions).toEqual([
      'What is X?',
      'How does Y work?',
    ])

    // done still triggered finishStreaming
    expect(state.isLoading).toBe(false)
    expect(assistant!.isStreaming).toBe(false)
  })

  it('preserves content + suggestions when session -> chunk -> suggestions -> done arrive in order', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-order' } },
          { event: 'chunk', data: { content: 'Hello' } },
          { event: 'chunk', data: { content: ' world' } },
          {
            event: 'suggestions',
            data: {
              suggestions: ['Follow-up A', 'Follow-up B', 'Follow-up C'],
            },
          },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const state = useChatStore.getState()
    const assistant = getLastAssistantMessage()

    // Regression: suggestions arrives AFTER chunks but BEFORE done —
    // the last assistant must carry BOTH accumulated content and suggestions,
    // and done must still close the stream. If detectEventType matched the
    // suggestions payload as 'done' (it has no sessionId/content/message),
    // the stream would short-circuit and suggestions would be lost.
    expect(assistant).toBeDefined()
    expect(assistant!.content).toBe('Hello world')
    expect(assistant!.suggestedQuestions).toEqual([
      'Follow-up A',
      'Follow-up B',
      'Follow-up C',
    ])
    expect(assistant!.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })

  it('leaves suggestedQuestions undefined when stream has no suggestions event', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-no-sugg' } },
          { event: 'chunk', data: { content: 'Plain answer' } },
          { event: 'done', data: {} },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    await result.current.sendMessage('test')

    const assistant = getLastAssistantMessage()

    expect(assistant).toBeDefined()
    expect(assistant!.content).toBe('Plain answer')
    // Default value is preserved (undefined) when no suggestions event fires
    expect(assistant!.suggestedQuestions).toBeUndefined()
    expect(assistant!.isStreaming).toBe(false)
  })
})
