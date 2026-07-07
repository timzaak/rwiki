import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import type { ValidatedWidgetConfig } from '@/widget/config'
import { FeedbackSubmitFnContext } from '@/hooks/feedback-context'
import type { FeedbackSubmitFn } from '@/hooks/feedback-context'

/**
 * FE-T01 — Widget feedback submitFn channelId contract test.
 *
 * Guards the FE-D01 invariant (widget-app.tsx feedbackSubmitFn): when a
 * feedback submission is triggered, the request body POSTed to
 * `/api/chat/feedback` MUST carry `channelId === config.channelId`, injected by the
 * widget before calling the generated `submitFeedback` SDK call.
 *
 * Mount strategy: STRATEGY C (probe via mocked child). `WidgetApp` is the only
 * export and does not accept children, so the real `WidgetAppContent`
 * (which creates `feedbackSubmitFn` and wraps the tree in
 * `FeedbackSubmitFnContext.Provider`) is exercised by mocking the heavy leaf
 * siblings — `FloatingButton` and `ChatModal` — and turning the `ChatModal`
 * mock into a probe that captures the real submitFn from
 * `FeedbackSubmitFnContext`. The probe therefore sits exactly where the real
 * feedback UI lives: inside the provider, capturing the real closure with
 * channelId injected. The test then invokes that captured submitFn directly.
 *
 * The generated SDK issues a real `fetch` (after `client.setConfig`) which MSW
 * intercepts; the load-bearing assertion reads the captured request body — we
 * never mock the internal `submitFeedback` SDK function.
 *
 * NOTE on choice: the real ChatModal feedback controls (like/dislike buttons)
 * have no stable testid in jsdom and require a seeded assistant message +
 * hover interactions that are fragile. Strategy A was rejected because
 * `WidgetApp` does not accept children, so a sibling probe cannot be placed
 * inside the provider. Strategy C (probe child) is the most stable way to
 * reach the real `feedbackSubmitFn` and assert on the MSW-captured body.
 */

const API_URL = 'http://localhost:3000'
const CHANNEL_ID = 'help-center'

/** A minimal valid config exercising the widget's real content component. */
function makeConfig(
  overrides: Partial<ValidatedWidgetConfig> = {},
): ValidatedWidgetConfig {
  return {
    apiUrl: API_URL,
    channelId: CHANNEL_ID,
    primaryColor: '#3b82f6',
    position: 'right',
    locale: 'en',
    ...overrides,
  }
}

/**
 * The body the real ChatModal would assemble for a "like" action. Only the
 * fields the widget forwards matter here; channelId is what the widget injects.
 */
const sampleFeedbackBody = {
  messageId: 'msg-1',
  sessionId: 'sess-1',
  userMessage: 'hello',
  assistantMessage: 'hi there',
  feedback: 'like',
}

describe('WidgetApp feedback submitFn (channelId in request body)', () => {
  let capturedBody: Record<string, unknown> | undefined
  // The probe writes the real widget-created submitFn here; the test reads
  // and invokes it directly (cleaner than driving a synthetic click through
  // act, and still exercises the real closure + real SDK fetch).
  let capturedSubmitFn: FeedbackSubmitFn | undefined

  beforeEach(() => {
    // Point the generated SDK at the MSW-intercepted base URL.
    client.setConfig({ baseUrl: API_URL })
    capturedBody = undefined
    capturedSubmitFn = undefined

    // MSW handler captures each POST /api/chat/feedback body in a closure.
    server.use(
      http.post(`${API_URL}/api/chat/feedback`, async ({ request }) => {
        capturedBody = (await request.json()) as Record<string, unknown>
        return HttpResponse.json({ ok: true })
      }),
      // The real WidgetAppContent also runs useWidgetSuggestions, which fires
      // GET /api/chat/suggestions. Provide a handler so it doesn't warn.
      http.get(`${API_URL}/api/chat/suggestions`, () =>
        HttpResponse.json({ questions: [] }),
      ),
    )
  })

  afterEach(() => {
    vi.doUnmock('@/components/chat/floating-button')
    vi.doUnmock('@/components/chat/chat-modal')
    vi.restoreAllMocks()
  })

  async function renderWidgetWithProbe(config: ValidatedWidgetConfig) {
    // Mock the heavy siblings; ChatModal becomes a probe that captures the
    // real submitFn from FeedbackSubmitFnContext.
    vi.doMock('@/components/chat/floating-button', () => ({
      FloatingButton: () => null,
    }))
    vi.doMock('@/components/chat/chat-modal', () => ({
      ChatModal: function ChatModalProbe() {
        return (
          <FeedbackSubmitFnContext.Consumer>
            {(submitFn) => <SubmitFnCapture submitFn={submitFn ?? undefined} />}
          </FeedbackSubmitFnContext.Consumer>
        )
      },
    }))

    // Dynamically import AFTER doMock so the mocks take effect for the
    // widget-app import graph.
    const { WidgetApp } = await import('../widget-app')

    return render(<WidgetApp config={config} />)
  }

  it('POST /api/chat/feedback body contains channelId === config.channelId', async () => {
    await renderWidgetWithProbe(makeConfig({ channelId: CHANNEL_ID }))

    // Wait for the probe to have captured the real widget submitFn.
    await waitFor(() => expect(capturedSubmitFn).toBeDefined())
    await capturedSubmitFn!(sampleFeedbackBody as any)

    await waitFor(() => {
      expect(capturedBody).toBeDefined()
    })

    // LOAD-BEARING: the widget must inject channelId into the feedback body.
    expect(capturedBody).toMatchObject({
      ...sampleFeedbackBody,
      channelId: CHANNEL_ID,
    })
  })

  it('uses the current config.channelId after re-render with a different channel', async () => {
    const OTHER_CHANNEL = 'docs-channel'
    const rendered = await renderWidgetWithProbe(makeConfig({ channelId: CHANNEL_ID }))

    await waitFor(() => expect(capturedSubmitFn).toBeDefined())
    await capturedSubmitFn!(sampleFeedbackBody as any)
    await waitFor(() => expect(capturedBody).toBeDefined())
    expect(capturedBody).toHaveProperty('channelId', CHANNEL_ID)

    // Re-render with a different channelId; the memoized submitFn must adopt it.
    capturedBody = undefined
    capturedSubmitFn = undefined
    const { WidgetApp } = await import('../widget-app')
    rendered.rerender(<WidgetApp config={makeConfig({ channelId: OTHER_CHANNEL })} />)

    await waitFor(() => expect(capturedSubmitFn).toBeDefined())
    await capturedSubmitFn!(sampleFeedbackBody as any)
    await waitFor(() => expect(capturedBody).toBeDefined())
    expect(capturedBody).toHaveProperty('channelId', OTHER_CHANNEL)
  })

  /** Captures the latest submitFn into the test closure on every render. */
  function SubmitFnCapture({ submitFn }: { submitFn?: FeedbackSubmitFn }) {
    capturedSubmitFn = submitFn
    return null
  }
})
