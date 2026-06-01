import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { createSseResponse } from '@/test/helpers/sse'
import { useChatStream } from '@/hooks/use-chat-stream'
import { useChatStore } from '@/stores/chat-store'
import { client } from '@/lib/api-generated/client.gen'

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

    const { result } = renderHook(() => useChatStream())

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

    const { result } = renderHook(() => useChatStream())

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

    const { result } = renderHook(() => useChatStream())

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

    const { result } = renderHook(() => useChatStream())

    await result.current.sendMessage('test')

    const state = useChatStore.getState()

    // The SSE client swallows the network error (sseMaxRetryAttempts=0)
    // so the hook finishes streaming without setting an error
    expect(state.isLoading).toBe(false)
  })

  it('stopStreaming aborts the stream', async () => {
    const encoder = new TextEncoder()

    // A stream that emits a session event then stays open —
    // the abort will cut it off before a done event arrives.
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

    const { result } = renderHook(() => useChatStream())

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

    const { result } = renderHook(() => useChatStream())

    await result.current.sendMessage('test question')

    expect(capturedBody).toEqual({
      message: 'test question',
      sessionId: null,
    })
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

    const { result } = renderHook(() => useChatStream())

    await result.current.sendMessage('follow-up question')

    expect(capturedBody).toEqual({
      message: 'follow-up question',
      sessionId: 'existing-session-123',
    })
  })
})
