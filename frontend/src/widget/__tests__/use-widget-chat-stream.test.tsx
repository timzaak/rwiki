import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'

import { useWidgetChatStream } from '../use-widget-chat-stream'
import { useChatStore } from '@/stores/chat-store'
import { createControllableSseResponse } from '@/test/helpers/sse'

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
const CHANNEL_ID = ['channel-a']

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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.error).toBe('Unable to connect to server. Please check your configuration or try again later.')
    expect(state.isLoading).toBe(false)
  })

  it('sets error with status code on non-OK HTTP response', async () => {
    fetchSpy.mockResolvedValue(new Response('Service Unavailable', { status: 503 }))

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    expect(state.error).toBe('Request failed (503)')
    expect(state.isLoading).toBe(false)
  })

  it('scenario 4: aborting before any content removes the empty assistant placeholder', async () => {
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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    const sendPromise = result.current.sendMessage('Hi')
    result.current.stopStreaming()
    await sendPromise

    await waitFor(() => {
      expect(useChatStore.getState().isLoading).toBe(false)
    })

    const state = useChatStore.getState()
    // No assistant bubble at all — an empty placeholder would be rendered
    // as a failed response by MessageItem's isFailed branch.
    expect(state.messages.filter((m) => m.role === 'assistant')).toHaveLength(0)
    expect(
      state.messages.some((m) => m.role === 'user' && m.content === 'Hi'),
    ).toBe(true)
    expect(state.error).toBeNull()
  })

  it('scenario 2: sending a new question mid-stream interrupts the old answer and does not clear the new stream loading', async () => {
    // First stream: emits session + partial content, stays open until aborted
    const firstFetch = vi.fn().mockImplementation(async (_url: string, init: RequestInit) => {
      const signal = init.signal ?? new AbortController().signal
      const stream = createAbortableStream(
        [
          sseData({ sessionId: 'sess-first' }),
          sseData({ content: 'Partial first' }),
        ],
        signal,
      )
      return new Response(stream, {
        headers: { 'Content-Type': 'text/event-stream' },
      })
    })

    // Second stream: delivers its content but only closes when the test
    // says so, pinning the loading state deterministically.
    const second = createControllableSseResponse([
      { event: 'session', data: { sessionId: 'sess-second' } },
      { event: 'chunk', data: { content: 'Second reply' } },
    ])
    const secondFetch = vi.fn().mockImplementation(async () => second.response)

    fetchSpy.mockImplementationOnce(firstFetch).mockImplementationOnce(secondFetch)

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    // Start first message (stream stays open) and wait for its content
    const firstPromise = result.current.sendMessage('First')
    await waitFor(() => {
      expect(firstFetch).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      const assistants = useChatStore
        .getState()
        .messages.filter((m) => m.role === 'assistant')
      expect(assistants[0]?.content).toBe('Partial first')
    })

    const secondPromise = result.current.sendMessage('Second')
    await firstPromise

    const state = useChatStore.getState()
    const assistants = state.messages.filter((m) => m.role === 'assistant')
    expect(assistants[0]).toMatchObject({
      content: 'Partial first',
      interrupted: true,
      isStreaming: false,
    })
    // Load-bearing ownsLoading assertion: the superseded stream's async
    // cleanup must not clear the loading state of the stream that replaced
    // it (isLoading must still be true — the second answer is streaming).
    expect(state.isLoading).toBe(true)

    second.enqueueEvent('done', {})
    second.close()
    await secondPromise

    const finalState = useChatStore.getState()
    expect(finalState.isLoading).toBe(false)
    const lastAssistant = finalState.messages.filter((m) => m.role === 'assistant').at(-1)
    expect(lastAssistant).toMatchObject({ content: 'Second reply', isStreaming: false })
    expect(lastAssistant?.interrupted).toBeUndefined()
  })
})

describe('useWidgetChatStream chat interruption (US-CORE-039)', () => {
  let fetchSpy: ReturnType<typeof vi.fn>

  /** Streams initial events, then errors the stream on abort. */
  function mockAbortableStream(initialEvents: string[]) {
    fetchSpy.mockImplementation(async (_url: string, init: RequestInit) => {
      const signal = init.signal ?? new AbortController().signal
      const stream = createAbortableStream(initialEvents, signal)
      return new Response(stream, {
        headers: { 'Content-Type': 'text/event-stream' },
      })
    })
  }

  beforeEach(() => {
    resetStore()
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  it('scenario 1: stopStreaming mid-stream keeps the partial answer and marks it interrupted without an error', async () => {
    mockAbortableStream([
      sseData({ sessionId: 'sess-stop' }),
      sseData({ content: 'Partial answer' }),
    ])

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    const sendPromise = result.current.sendMessage('Hi')
    // Stop only after the partial content is on screen — that is when the
    // user actually clicks stop.
    await waitFor(() => {
      const last = useChatStore.getState().messages.at(-1)
      expect(last?.role).toBe('assistant')
      expect(last?.content).toBe('Partial answer')
    })
    result.current.stopStreaming()
    await sendPromise

    await waitFor(() => {
      expect(useChatStore.getState().isLoading).toBe(false)
    })

    const state = useChatStore.getState()
    const assistant = state.messages.filter((m) => m.role === 'assistant').at(-1)
    expect(assistant).toMatchObject({
      content: 'Partial answer',
      interrupted: true,
      isStreaming: false,
    })
    // User interruption is not an error.
    expect(state.error).toBeNull()
  })

  it('scenario 3: aborting after contentEnd keeps the answer complete without the interrupted flag', async () => {
    mockAbortableStream([
      sseData({ sessionId: 'sess-content-end' }),
      sseData({ content: 'Complete answer' }),
      sseData({ contentEnd: true }),
    ])

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    const sendPromise = result.current.sendMessage('Hi')
    await waitFor(() => {
      const last = useChatStore.getState().messages.at(-1)
      expect(last?.contentEnded).toBe(true)
    })
    result.current.stopStreaming()
    await sendPromise

    await waitFor(() => {
      expect(useChatStore.getState().isLoading).toBe(false)
    })

    const state = useChatStore.getState()
    const assistant = state.messages.filter((m) => m.role === 'assistant').at(-1)
    // The answer body had already finished (suggestion-generation phase):
    // it must be kept complete, not flagged as possibly-incomplete.
    expect(assistant).toMatchObject({
      content: 'Complete answer',
      contentEnded: true,
      isStreaming: false,
    })
    expect(assistant?.interrupted).toBeUndefined()
    expect(state.error).toBeNull()
  })

  it('ignores stale buffered events a non-spec host fetch resolves after abort', async () => {
    // WHY: Widget 嵌入任意宿主页；宿主 polyfill 的 fetch 在 abort 后可能
    // 仍用已缓冲数据 resolve read()。旧轮次残留 chunk 会按"最后一条
    // assistant"寻址追加进新轮次占位（跨轮串染），残留 done 会提前复位
    // 新轮次的 isLoading——读循环必须逐次复查 abort。
    const first = createControllableSseResponse([
      { event: 'session', data: { sessionId: 'sess-first' } },
      { event: 'chunk', data: { content: 'Partial first' } },
    ])
    const second = createControllableSseResponse([
      { event: 'session', data: { sessionId: 'sess-second' } },
      { event: 'chunk', data: { content: 'Second reply' } },
    ])
    fetchSpy.mockImplementationOnce(async () => first.response)
    fetchSpy.mockImplementationOnce(async () => second.response)

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    const firstPromise = result.current.sendMessage('First')
    await waitFor(() => {
      const assistants = useChatStore
        .getState()
        .messages.filter((m) => m.role === 'assistant')
      expect(assistants[0]?.content).toBe('Partial first')
    })

    // Implicit interrupt: the second send aborts the first controller, but
    // the first stream still resolves its pending read with stale events —
    // exactly what a non-spec fetch host produces.
    const secondPromise = result.current.sendMessage('Second')
    first.enqueueEvent('chunk', { content: 'STALE' })
    first.enqueueEvent('done', {})
    await firstPromise
    await waitFor(() => {
      const assistants = useChatStore
        .getState()
        .messages.filter((m) => m.role === 'assistant')
      expect(assistants[1]?.content).toBe('Second reply')
    })

    const state = useChatStore.getState()
    const assistants = state.messages.filter((m) => m.role === 'assistant')
    expect(assistants[0]).toMatchObject({
      content: 'Partial first',
      interrupted: true,
      isStreaming: false,
    })
    // No cross-round bleed: the stale chunk never reached the new placeholder.
    expect(assistants[1]).toMatchObject({ content: 'Second reply' })
    // The stale done did not finish the new round.
    expect(state.isLoading).toBe(true)

    second.enqueueEvent('done', {})
    second.close()
    await secondPromise

    const finalState = useChatStore.getState()
    expect(finalState.isLoading).toBe(false)
    expect(
      finalState.messages.filter((m) => m.role === 'assistant').at(-1),
    ).toMatchObject({ content: 'Second reply', isStreaming: false })
  })

  // CRITICAL REGRESSION: if `case 'contentEnd'` in processSseLines were
  // `return true` instead of `break`, everything after contentEnd would never
  // reach the store and `done` would never fire finishStreaming. This test
  // MUST fail in that case — it is the load-bearing assertion.
  it('does not terminate the stream on contentEnd; chunk/suggestions after contentEnd are still processed and done still finishes streaming', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([
        sseData({ sessionId: 'sess-ce-order' }),
        sseData({ content: 'before-' }),
        sseData({ contentEnd: true }),
        sseData({ content: 'after' }),
        sseData({ suggestions: ['follow-up-1', 'follow-up-2'] }),
        sseData({}),
      ]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    await result.current.sendMessage('Hi')

    const state = useChatStore.getState()
    const lastMsg = state.messages[state.messages.length - 1]
    expect(lastMsg.role).toBe('assistant')
    // post-contentEnd chunk content MUST be present (would be missing if
    // contentEnd returned true and broke out of the outer while loop)
    expect(lastMsg.content).toBe('before-after')
    expect(lastMsg.contentEnded).toBe(true)
    expect(lastMsg.suggestedQuestions).toEqual(['follow-up-1', 'follow-up-2'])
    // done MUST have fired finishStreaming (would stay true if contentEnd
    // returned true and skipped done)
    expect(lastMsg.isStreaming).toBe(false)
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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

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

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

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

describe('useWidgetChatStream channelId passthrough (request body contract)', () => {
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

  it('sends channelId in the /api/chat POST body alongside message + sessionId', async () => {
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-body' }), sseData({})]),
    )

    const { result } = renderHook(() => useWidgetChatStream(API_URL, CHANNEL_ID))

    await result.current.sendMessage('Hi')

    const url = fetchSpy.mock.calls[0]![0] as string
    expect(url).toBe(`${API_URL}/api/chat`)

    // The JSON body carries channelId at the same level as message/sessionId
    const body = readBody(0)
    expect(body).toEqual(
      expect.objectContaining({
        message: 'Hi',
        sessionId: null, // no prior session
        channelId: CHANNEL_ID,
        // Capability negotiation: declaring it gates the server-side
        // contentEnd event; without it the server must never send one.
        supportsContentEndEvent: true,
      }),
    )
  })

  it('uses the current channelId when re-rendered with a different channelId', async () => {
    // Guards against channelId being hoisted/omitted in the sendMessage closure.
    const OTHER_CHANNEL = ['channel-b']
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-a' }), sseData({})]),
    )

    const { result, rerender } = renderHook(
      ({ channelId }) => useWidgetChatStream(API_URL, channelId),
      { initialProps: { channelId: CHANNEL_ID } },
    )

    await result.current.sendMessage('first')

    // Re-render with a different channelId; the memoized sendMessage must pick it up
    rerender({ channelId: OTHER_CHANNEL })
    fetchSpy.mockClear()
    fetchSpy.mockResolvedValue(
      createSseResponse([sseData({ sessionId: 'sess-b' }), sseData({})]),
    )

    await result.current.sendMessage('second')

    const body = readBody(0)
    expect(body.channelId).toEqual(OTHER_CHANNEL)
    expect(body).not.toHaveProperty('channelId', CHANNEL_ID)
  })
})
