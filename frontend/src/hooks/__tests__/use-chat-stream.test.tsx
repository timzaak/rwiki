import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import {
  createControllableSseResponse,
  createHangingSseResponse,
  createSseResponse,
} from '@/test/helpers/sse'
import { useChatStream } from '@/hooks/use-chat-stream'
import { useChatStore } from '@/stores/chat-store'
import { client } from '@/lib/api-generated/client.gen'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'

/**
 * Regression: `useChatStream` now reads `useChannelId()` at the top, so
 * every hook render must be wrapped in `ChannelIdProvider` or it throws. The
 * wrappers below inject the main-site channelId that flows into the chat request
 * body (`channelId` alongside `message`/`sessionId`).
 */
function makeWrapper(channelId: string[] = ['channel-a']) {
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

    expect(state.messages.some((m) => m.role === 'user' && m.content === 'What is the sales data?')).toBe(true)

    const assistant = getLastAssistantMessage()
    expect(assistant).toBeDefined()
    expect(assistant!.content).toBe('Hello from AI')

    expect(state.sessionId).toBe('sess-abc')

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

    const assistant = getLastAssistantMessage()
    expect(assistant!.content).toBe('Partial content')

    expect(state.error).toBe('Internal server error')
    expect(state.isLoading).toBe(false)
    // A network/server error must NOT look like a user interruption —
    // the retry affordance depends on this distinction.
    expect(getLastAssistantMessage()?.interrupted).toBeUndefined()
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
})

describe('useChatStream chat interruption (US-CORE-039)', () => {
  beforeEach(() => {
    resetStore()
  })

  it('scenario 1: stopStreaming mid-stream keeps the partial answer and marks it interrupted without an error', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createHangingSseResponse([
          { event: 'session', data: { sessionId: 'sess-stop' } },
          { event: 'chunk', data: { content: 'Partial answer' } },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    const sendPromise = result.current.sendMessage('test')
    // Stop only after the partial content is on screen — that is when the
    // user actually clicks stop.
    await waitFor(() => {
      expect(getLastAssistantMessage()?.content).toBe('Partial answer')
    })
    result.current.stopStreaming()
    await sendPromise

    const state = useChatStore.getState()
    const assistant = getLastAssistantMessage()
    expect(assistant?.content).toBe('Partial answer')
    expect(assistant?.interrupted).toBe(true)
    expect(assistant?.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
    // User interruption is not an error: no error banner, no retry affordance.
    expect(state.error).toBeNull()
  })

  it('scenario 2: sending a new question mid-stream interrupts the old answer and does not clear the new stream loading', async () => {
    // The replacement stream delivers its content but only closes when the
    // test says so, pinning the loading state deterministically.
    const second = createControllableSseResponse([
      { event: 'session', data: { sessionId: 'sess-second' } },
      { event: 'chunk', data: { content: 'Second reply' } },
    ])

    let call = 0
    server.use(
      http.post('/api/chat', () => {
        call += 1
        if (call === 1) {
          return createHangingSseResponse([
            { event: 'session', data: { sessionId: 'sess-first' } },
            { event: 'chunk', data: { content: 'Partial first' } },
          ])
        }
        return second.response
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    const firstPromise = result.current.sendMessage('First')
    await waitFor(() => {
      expect(getLastAssistantMessage()?.content).toBe('Partial first')
    })

    const secondPromise = result.current.sendMessage('Second')
    await firstPromise

    const assistants = useChatStore
      .getState()
      .messages.filter((m) => m.role === 'assistant')
    expect(assistants[0]).toMatchObject({
      content: 'Partial first',
      interrupted: true,
      isStreaming: false,
    })
    // Load-bearing ownsLoading assertion: the superseded stream's async
    // cleanup must not clear the loading state of the stream that replaced
    // it (isLoading must still be true — the second answer is streaming).
    expect(useChatStore.getState().isLoading).toBe(true)

    second.enqueueEvent('done', {})
    second.close()
    await secondPromise

    const state = useChatStore.getState()
    expect(state.isLoading).toBe(false)
    const last = getLastAssistantMessage()
    expect(last).toMatchObject({ content: 'Second reply', isStreaming: false })
    expect(last?.interrupted).toBeUndefined()
  })

  it('scenario 3: aborting after contentEnd keeps the answer complete without the interrupted flag', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createHangingSseResponse([
          { event: 'session', data: { sessionId: 'sess-content-end' } },
          { event: 'chunk', data: { content: 'Complete answer' } },
          { event: 'contentEnd', data: { contentEnd: true } },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    const sendPromise = result.current.sendMessage('test')
    await waitFor(() => {
      expect(getLastAssistantMessage()?.contentEnded).toBe(true)
    })
    result.current.stopStreaming()
    await sendPromise

    const state = useChatStore.getState()
    const assistant = getLastAssistantMessage()
    // The answer body had already finished (suggestion-generation phase):
    // it must be kept complete, not flagged as possibly-incomplete.
    expect(assistant?.content).toBe('Complete answer')
    expect(assistant?.contentEnded).toBe(true)
    expect(assistant?.interrupted).toBeUndefined()
    expect(assistant?.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
    expect(state.error).toBeNull()
  })

  it('scenario 4: aborting before any content removes the empty assistant placeholder', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createHangingSseResponse([
          { event: 'session', data: { sessionId: 'sess-empty' } },
        ])
      }),
    )

    const { result } = renderHook(() => useChatStream(), {
      wrapper: makeWrapper(),
    })

    const sendPromise = result.current.sendMessage('test')
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
      state.messages.some((m) => m.role === 'user' && m.content === 'test'),
    ).toBe(true)
    expect(state.error).toBeNull()
  })

  // CRITICAL REGRESSION: if `contentEnd` were misdetected as `done` by
  // detectEventType, the stream would terminate early — suggestions would be
  // lost and the turn would look complete without its follow-up chips.
  it('does not terminate the stream on contentEnd; suggestions after contentEnd are still written and done still finishes streaming', async () => {
    server.use(
      http.post('/api/chat', () => {
        return createSseResponse([
          { event: 'session', data: { sessionId: 'sess-ce-order' } },
          { event: 'chunk', data: { content: 'Final answer' } },
          { event: 'contentEnd', data: { contentEnd: true } },
          { event: 'suggestions', data: { suggestions: ['q1', 'q2'] } },
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
    expect(assistant?.content).toBe('Final answer')
    expect(assistant?.contentEnded).toBe(true)
    expect(assistant?.suggestedQuestions).toEqual(['q1', 'q2'])
    expect(assistant?.isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
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
      // channelId from ChannelIdProvider is transmitted in the chat request
      // body alongside message/sessionId.
      channelId: ['channel-a'],
      // Capability negotiation: declaring it gates the server-side contentEnd
      // event; without it the server must never send one.
      supportsContentEndEvent: true,
    })
  })

  it('changes body.channelId when the provider channelId changes on rerender', async () => {
    // Guards against channelId being hoisted out of the sendMessage dependency
    // array: if the hook closed over a stale channelId, the rerendered send would
    // still carry the old value. The wrapper reads a mutable ref so the same
    // wrapper instance can supply a different channelId after `rerender`.
    let capturedBody: unknown = null
    let currentChannelId: string[] = ['channel-a']

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

    currentChannelId = ['channel-b']
    rerender()

    await result.current.sendMessage('after rerender')

    const body = capturedBody as { channelId?: string[] } | null
    expect(body).not.toBeNull()
    expect(body!.channelId).toEqual(['channel-b'])
  })

  it('sends sessionId in request when store has one', async () => {
    let capturedBody: unknown = null

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
      // channelId is transmitted on follow-up messages too.
      channelId: ['channel-a'],
      supportsContentEndEvent: true,
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

    expect(assistant).toBeDefined()
    expect(assistant!.suggestedQuestions).toEqual([
      'What is X?',
      'How does Y work?',
    ])

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
    expect(assistant!.suggestedQuestions).toBeUndefined()
    expect(assistant!.isStreaming).toBe(false)
  })
})
