import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'

import { useWidgetChatStream } from '../use-widget-chat-stream'
import { useChatStore } from '@/stores/chat-store'

/**
 * Creates a mock SSE Response with raw event strings.
 * Each string should be a complete SSE event block (e.g. "data: {...}\\n\\n").
 */
function createSseResponse(events: string[]): Response {
  const encoder = new TextEncoder()
  const stream = new ReadableStream({
    start(controller) {
      for (const event of events) {
        controller.enqueue(encoder.encode(event))
      }
      controller.close()
    },
  })
  return new Response(stream, {
    headers: { 'Content-Type': 'text/event-stream' },
  })
}

/** Helper to build a data-only SSE block (no event line, matching hook behavior). */
function sseData(data: unknown): string {
  return `data: ${JSON.stringify(data)}\n\n`
}

/**
 * Creates a stream that emits initial events, then delays indefinitely
 * until the signal is aborted. This allows testing abort behavior because
 * the stream cancels the reader when the abort signal fires.
 */
function createAbortableStream(
  initialEvents: string[],
  signal: AbortSignal,
): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder()
  return new ReadableStream({
    start(controller) {
      for (const event of initialEvents) {
        controller.enqueue(encoder.encode(event))
      }
      // When abort fires, error the stream to unblock reader.read()
      signal.addEventListener('abort', () => {
        controller.error(new DOMException('The operation was aborted.', 'AbortError'))
      })
    },
  })
}

function resetStore() {
  useChatStore.setState({
    messages: [],
    sessionId: null,
    error: null,
    isLoading: false,
  })
}

const API_URL = 'http://localhost:3000'
const SITE_ID = 'site-a'

describe('useWidgetChatStream', () => {
  let fetchSpy: ReturnType<typeof vi.fn>

  beforeEach(() => {
    resetStore()
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('processes successful SSE stream: session -> setSessionId, chunk -> appendToLastAssistant, done -> finishStreaming', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-1' }),
        sseData({ content: 'Hello ' }),
        sseData({ content: 'world' }),
        sseData({}),
      ]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.sessionId).toBe('sess-1')

    const lastMsg = state.messages[state.messages.length - 1]
    expect(lastMsg.role).toBe('assistant')
    expect(lastMsg.content).toBe('Hello world')
    expect(lastMsg.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })

  it('sets error in store on network failure during fetch', async () => {
    fetchSpy.mockRejectedValue(new TypeError('Failed to fetch'))

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.error).toBe('Unable to connect to server. Please check your configuration or try again later.')
    expect(state.isLoading).toBe(false)
  })

  it('sets error with status code on non-OK HTTP response', async () => {
    fetchSpy.mockResolvedValue(new Response('Service Unavailable', { status: 503 }))

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.error).toBe('Request failed (503)')
    expect(state.isLoading).toBe(false)
  })

  it('finishes streaming without error when aborted', async () => {
    fetchSpy.mockImplementation(async (_url: string, init: RequestInit) => {
      const signal = init.signal ?? new AbortController().signal
      const stream = createAbortableStream(
        [sseData({ sessionId: 'sess-abort' })],
        signal,
      )
      return new Response(stream, {
        headers: { 'Content-Type': 'text/event-stream' },
      })
    })

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    // Start sending, then abort
    const sendPromise = result.current.sendMessage('Hi')
    result.current.stopStreaming()
    await sendPromise

    await waitFor(() => {
      expect(useChatStore.getState().isLoading).toBe(false)
    })

    const state = useChatStore.getState()
    expect(state.error).toBeNull()
  })

  it('aborts previous stream when sending a new message sequentially', async () => {
    // First stream: emits session, stays open until aborted
    const firstFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const signal = init.signal ?? new AbortController().signal
      const stream = createAbortableStream(
        [sseData({ sessionId: 'sess-first' })],
        signal,
      )
      return new Response(stream, {
        headers: { 'Content-Type': 'text/event-stream' },
      })
    })

    // Second stream: emits full response then closes
    const secondFetch = vi.fn().mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-second' }),
        sseData({ content: 'Reply' }),
        sseData({}),
      ]),
    )

    fetchSpy.mockImplementationOnce(firstFetch).mockImplementationOnce(secondFetch)

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    // Start first message (stream stays open)
    const firstPromise = result.current.sendMessage('First')

    // Wait for first fetch to be called
    await waitFor(() => {
      expect(firstFetch).toHaveBeenCalledTimes(1)
    })

    // Send second message — this aborts the first stream
    const secondPromise = result.current.sendMessage('Second')

    await firstPromise
    await secondPromise

    const state = useChatStore.getState()
    expect(state.sessionId).toBe('sess-second')

    const assistantMessages = state.messages.filter((m) => m.role === 'assistant')
    // At least the second assistant message exists
    expect(assistantMessages.length).toBeGreaterThanOrEqual(1)
    const lastAssistant = assistantMessages[assistantMessages.length - 1]
    expect(lastAssistant.content).toBe('Reply')
    expect(state.isLoading).toBe(false)
  })
})

describe('useWidgetChatStream post-answer suggestions', () => {
  let fetchSpy: ReturnType<typeof vi.fn>

  beforeEach(() => {
    resetStore()
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('parses suggestions event and writes to last assistant suggestedQuestions', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-sugg' }),
        sseData({ content: 'Answer' }),
        sseData({ suggestions: ['q1', 'q2'] }),
        sseData({}),
      ]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    const lastMsg = state.messages[state.messages.length - 1]
    expect(lastMsg.role).toBe('assistant')
    expect(lastMsg.suggestedQuestions).toEqual(['q1', 'q2'])
    expect(lastMsg.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })

  // CRITICAL REGRESSION: if `case 'suggestions'` in processSseLines were
  // `return true` instead of `break`, the post-suggestions chunk would never
  // reach the store and `done` would never fire finishStreaming. This test
  // MUST fail in that case — it is the load-bearing assertion.
  it('does not terminate the stream on suggestions; chunk after suggestions is still accumulated and done still finishes streaming', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-order' }),
        sseData({ content: 'before-' }),
        sseData({ suggestions: ['follow-up-1', 'follow-up-2'] }),
        sseData({ content: 'after' }),
        sseData({}),
      ]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    const lastMsg = state.messages[state.messages.length - 1]
    expect(lastMsg.role).toBe('assistant')
    // post-suggestions chunk content MUST be present (would be missing if
    // suggestions returned true and broke out of the outer while loop)
    expect(lastMsg.content).toBe('before-after')
    expect(lastMsg.suggestedQuestions).toEqual(['follow-up-1', 'follow-up-2'])
    // done MUST have fired finishStreaming (would stay true if suggestions
    // returned true and skipped done)
    expect(lastMsg.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })

  it('preserves content + suggestions when session -> chunk -> suggestions -> chunk -> done arrive in order', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-preserve' }),
        sseData({ content: 'Hello ' }),
        sseData({ content: 'world' }),
        sseData({ suggestions: ['what-next-a', 'what-next-b'] }),
        sseData({ content: '!' }),
        sseData({}),
      ]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.sessionId).toBe('sess-preserve')

    const lastMsg = state.messages[state.messages.length - 1]
    expect(lastMsg.role).toBe('assistant')
    expect(lastMsg.content).toBe('Hello world!')
    expect(lastMsg.suggestedQuestions).toEqual(['what-next-a', 'what-next-b'])
    expect(lastMsg.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })
})

describe('useWidgetChatStream siteId passthrough (request body contract)', () => {
  let fetchSpy: ReturnType<typeof vi.fn>

  beforeEach(() => {
    resetStore()
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  /** Reads the JSON body of the nth fetch call (init arg index 1). */
  function readBody(callIndex = 0): Record<string, unknown> {
    const init = fetchSpy.mock.calls[callIndex]![1] as RequestInit
    return JSON.parse(init.body as string)
  }

  it('sends siteId in the /api/chat POST body alongside message + sessionId', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-body' }), sseData({})]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, SITE_ID))

    await result.current.sendMessage('Hi')

    // The fetch hit /api/chat ...
    const url = fetchSpy.mock.calls[0]![0] as string
    expect(url).toBe(`${API_URL}/api/chat`)

    // ... and its JSON body carries siteId at the same level as message/sessionId
    const body = readBody(0)
    expect(body).toEqual(
      expect.objectContaining({
        message: 'Hi',
        sessionId: null, // no prior session
        siteId: SITE_ID,
      }),
    )
  })

  it('uses the current siteId when re-rendered with a different siteId', async () => {
    // Guards against siteId being hoisted/omitted in the sendMessage closure.
    const OTHER_SITE = 'site-b'
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-a' }), sseData({})]),
    )

    const { result, rerender } = renderHook(
      ({ siteId }) => useWidgetChatStream(API_URL, siteId),
      { initialProps: { siteId: SITE_ID } },
    )

    await result.current.sendMessage('first')

    // Re-render with a different siteId; the memoized sendMessage must pick it up
    rerender({ siteId: OTHER_SITE })
    fetchSpy.mockClear()
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-b' }), sseData({})]),
    )

    await result.current.sendMessage('second')

    const body = readBody(0)
    expect(body.siteId).toBe(OTHER_SITE)
    expect(body).not.toHaveProperty('siteId', SITE_ID)
  })
})
