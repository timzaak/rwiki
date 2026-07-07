import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { useFeedback } from '@/hooks/use-feedback'
import type { UseFeedbackOptions } from '@/hooks/use-feedback'
import { useChatStore } from '@/stores/chat-store'
import type { ChatMessage } from '@/stores/chat-store'
import { FeedbackSubmitFnContext } from '@/hooks/feedback-context'
import { client } from '@/lib/api-generated/client.gen'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'

/**
 * FE-D02 regression: `useFeedback` now reads `useChannelId()` at the top, so every
 * hook render must be wrapped in `ChannelIdProvider` or it throws. The wrapper
 * injects the main-site channelId that flows into the feedback request body
 * (`channelId` alongside sessionId/messageId/feedback).
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

function makeMessage(overrides?: Partial<ChatMessage>): ChatMessage {
  return {
    id: 'msg-assistant-1',
    role: 'assistant',
    content: 'AI response',
    timestamp: Date.now(),
    ...overrides,
  }
}

function makeFeedbackOptions(
  overrides?: Partial<UseFeedbackOptions>,
): UseFeedbackOptions {
  return {
    sessionId: 'session-1',
    messageId: 'msg-assistant-1',
    userMessage: 'user question',
    assistantMessage: 'AI response',
    ...overrides,
  }
}

function seedStore(messages: ChatMessage[], sessionId: string | null = 'session-1') {
  useChatStore.setState({
    messages,
    sessionId,
    isLoading: false,
    error: null,
  })
}

// The generated SDK client requires an absolute URL in the MSW/Node.js test
// environment. Set baseUrl so the POST to /api/chat/feedback resolves correctly.
beforeEach(() => {
  resetStore()
  client.setConfig({ baseUrl: 'http://localhost:3000' })
})

describe('useFeedback initial state', () => {
  it('returns undefined feedback when message has no feedback', () => {
    seedStore([makeMessage({ feedback: undefined })])

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    expect(result.current.feedback).toBeUndefined()
  })

  it('returns like feedback when message feedback is like', () => {
    seedStore([makeMessage({ feedback: 'like' })])

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    expect(result.current.feedback).toBe('like')
  })

  it('returns dislike feedback when message feedback is dislike', () => {
    seedStore([makeMessage({ feedback: 'dislike' })])

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    expect(result.current.feedback).toBe('dislike')
  })

  it('returns isSubmitting false initially', () => {
    seedStore([makeMessage()])

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    expect(result.current.isSubmitting).toBe(false)
  })
})

describe('useFeedback submitFeedback', () => {
  it('optimistically updates store and calls API on submit', async () => {
    seedStore([makeMessage({ feedback: undefined })])

    let capturedBody: unknown = null
    server.use(
      http.post('/api/chat/feedback', async ({ request }) => {
        capturedBody = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    await act(async () => {
      await result.current.submitFeedback('like')
    })

    // Store updated optimistically
    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBe('like')

    // API was called with correct body
    expect(capturedBody).toEqual({
      sessionId: 'session-1',
      messageId: 'msg-assistant-1',
      feedback: 'like',
      userMessage: 'user question',
      assistantMessage: 'AI response',
      // FE-T02 load-bearing contract: channelId from ChannelIdProvider is transmitted
      // in the feedback request body alongside the feedback fields.
      channelId: ['channel-a'],
    })

    // isSubmitting back to false after completion
    expect(result.current.isSubmitting).toBe(false)
  })

  it('toggles from like to dislike', async () => {
    seedStore([makeMessage({ feedback: 'like' })])

    let capturedBody: unknown = null
    server.use(
      http.post('/api/chat/feedback', async ({ request }) => {
        capturedBody = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    await act(async () => {
      await result.current.submitFeedback('dislike')
    })

    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBe('dislike')
    expect((capturedBody as { feedback: string }).feedback).toBe('dislike')
  })

  it('includes channelId in the feedback request body (dislike path)', async () => {
    // FE-T02 load-bearing contract: triggering like/dislike on the main-site
    // path (no contextSubmitFn → SDK branch) must carry channelId in the body,
    // not just the feedback fields.
    seedStore([makeMessage({ feedback: undefined })])

    let capturedBody: unknown = null
    server.use(
      http.post('/api/chat/feedback', async ({ request }) => {
        capturedBody = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(['channel-a']),
    })

    await act(async () => {
      await result.current.submitFeedback('dislike')
    })

    const body = capturedBody as { channelId?: string[]; feedback?: string } | null
    expect(body).not.toBeNull()
    expect(body!.channelId).toEqual(['channel-a'])
    expect(body!.feedback).toBe('dislike')
  })

  it('cancels feedback when submitting same type', async () => {
    seedStore([makeMessage({ feedback: 'like' })])

    let capturedBody: unknown = null
    server.use(
      http.post('/api/chat/feedback', async ({ request }) => {
        capturedBody = await request.json()
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    await act(async () => {
      await result.current.submitFeedback('like')
    })

    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBeUndefined()
    expect((capturedBody as { feedback: string | null }).feedback).toBeNull()
  })

  it('rolls back to previous state on API error', async () => {
    seedStore([makeMessage({ feedback: 'like' })])

    // The generated SDK uses submitFeedback<true>({ body }) which is a
    // type-level ThrowOnError — it does NOT set throwOnError at runtime.
    // Enable it explicitly so the 500 response triggers the hook's catch block.
    client.setConfig({ baseUrl: 'http://localhost:3000', throwOnError: true })

    server.use(
      http.post('/api/chat/feedback', () =>
        HttpResponse.json(
          { code: 500, message: 'Database error' },
          { status: 500 },
        ),
      ),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    await act(async () => {
      await result.current.submitFeedback('dislike')
    })

    // Should roll back to the original 'like' state
    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBe('like')
    expect(result.current.isSubmitting).toBe(false)
  })

  it('sets isSubmitting true during submit', async () => {
    seedStore([makeMessage({ feedback: undefined })])

    let resolveResponse: () => void
    const responsePromise = new Promise<void>(
      (resolve) => (resolveResponse = resolve),
    )

    server.use(
      http.post('/api/chat/feedback', async () => {
        await responsePromise
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    // Start the submit
    const submitPromise = act(async () => {
      await result.current.submitFeedback('like')
    })

    // Wait for isSubmitting to become true
    await waitFor(() => {
      expect(result.current.isSubmitting).toBe(true)
    })

    // Resolve the delayed response
    resolveResponse!()
    await submitPromise

    expect(result.current.isSubmitting).toBe(false)
  })

  it('returns early when sessionId is null', async () => {
    seedStore([makeMessage()], null)
    // sessionId is null via seedStore

    let handlerReached = false
    server.use(
      http.post('/api/chat/feedback', () => {
        handlerReached = true
        return new HttpResponse(null, { status: 204 })
      }),
    )

    const { result } = renderHook(
      () => useFeedback(makeFeedbackOptions({ sessionId: null })),
      { wrapper: makeWrapper() },
    )

    await act(async () => {
      await result.current.submitFeedback('like')
    })

    expect(handlerReached).toBe(false)
    expect(result.current.isSubmitting).toBe(false)
  })
})

describe('useFeedback context resolution', () => {
  it('uses context submitFn when FeedbackSubmitFnContext is provided', async () => {
    seedStore([makeMessage({ feedback: undefined })])

    const contextSubmitFn = vi.fn().mockResolvedValue(undefined)

    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <ChannelIdProvider channelId={['channel-a']}>
        <FeedbackSubmitFnContext.Provider value={contextSubmitFn}>
          {children}
        </FeedbackSubmitFnContext.Provider>
      </ChannelIdProvider>
    )

    const { result } = renderHook(
      () => useFeedback(makeFeedbackOptions()),
      { wrapper },
    )

    await act(async () => {
      await result.current.submitFeedback('like')
    })

    expect(contextSubmitFn).toHaveBeenCalledWith({
      sessionId: 'session-1',
      messageId: 'msg-assistant-1',
      feedback: 'like',
      userMessage: 'user question',
      assistantMessage: 'AI response',
      // channelId from the wrapping ChannelIdProvider is forwarded to the context
      // submitFn (Widget path); main-site path forwards it to the SDK body.
      channelId: ['channel-a'],
    })

    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBe('like')
  })

  it('falls through to SDK when no context provider', async () => {
    seedStore([makeMessage({ feedback: undefined })])

    let sdkCalled = false
    server.use(
      http.post('/api/chat/feedback', async () => {
        sdkCalled = true
        return new HttpResponse(null, { status: 204 })
      }),
    )

    // No FeedbackSubmitFnContext wrapper — falls through to SDK
    const { result } = renderHook(() => useFeedback(makeFeedbackOptions()), {
      wrapper: makeWrapper(),
    })

    await act(async () => {
      await result.current.submitFeedback('like')
    })

    expect(sdkCalled).toBe(true)

    const msg = useChatStore.getState().messages[0]
    expect(msg.feedback).toBe('like')
  })
})
