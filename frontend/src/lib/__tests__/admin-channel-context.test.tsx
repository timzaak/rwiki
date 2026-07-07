import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { type ReactNode } from 'react'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import {
  AdminChannelProvider,
  useAdminChannel,
} from '@/lib/admin-channel-context'

/**
 * FE-T03 — AdminChannelProvider global channel context tests
 *
 * Covers FE-D03's `admin-channel-context.tsx`:
 *  - default-first: on success with a non-empty list, `channelId` becomes the first
 *    channel's id (`channels[0].id`), `loading===false`, `error===null`, `channels`
 *    populated.
 *  - switch: `setChannelId('channel-b')` updates `channelId`.
 *  - empty: success with `channels: []` → `channelId===null` (consumers disable).
 *  - fail + retry: 500 → `error` non-null, `channelId===null`; `retry()` re-requests
 *    `/api/channels`; swapping to a 200 handler → after retry `channelId==='channel-a'`,
 *    `error===null`.
 *
 * Strategy mirrors `use-feedback.test.tsx` / `api-client-setup.test.tsx`:
 * `renderHook(... , { wrapper })` + MSW `/api/channels` + `client.setConfig({
 * baseUrl })` so the generated SDK's fetch reaches MSW. `listChannels` is NOT
 * mocked — the real SDK fetch flows through MSW.
 *
 * We do NOT assert the exact localStorage key (an implementation detail). The
 * provider auto-retries on failure with a delay; tests drive recovery via the
 * manual `retry()` (which cancels pending timers and re-loads immediately) and
 * only assert the externally observable contract.
 */

const BASE_URL = 'http://localhost:3000'
const CHANNELS_URL = `${BASE_URL}/api/channels`

const CHANNEL_A = { id: 'channel-a', name: 'A' }
const CHANNEL_B = { id: 'channel-b', name: 'B' }

// renderHook wrapper: AdminChannelProvider owns listChannels + the context value.
function wrapper({ children }: { children: ReactNode }) {
  return <AdminChannelProvider>{children}</AdminChannelProvider>
}

let channelsCallCount: number

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  localStorage.clear()
  channelsCallCount = 0
})

afterEach(() => {
  // Ensure fake timers (used by the failure/retry suite) never leak into the
  // other suites, which rely on real timers + waitFor.
  vi.useRealTimers()
})

function channelsHandler(
  body: Record<string, unknown>,
  status: 200 | 500 = 200,
): ReturnType<typeof http.get> {
  return http.get(CHANNELS_URL, () => {
    channelsCallCount += 1
    if (status === 500) {
      return HttpResponse.json(
        { code: 500, message: 'boom' },
        { status: 500 },
      )
    }
    return HttpResponse.json(body)
  })
}

describe('AdminChannelProvider — default-first selection', () => {
  it('defaults channelId to the first channel and clears loading on success', async () => {
    server.use(channelsHandler({ channels: [CHANNEL_A, CHANNEL_B] }))

    const { result } = renderHook(() => useAdminChannel(), { wrapper })

    await waitFor(() => {
      expect(result.current.channelId).toBe('channel-a')
    })

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(result.current.channels).toHaveLength(2)
    expect(result.current.channels.map((c) => c.id)).toEqual([
      'channel-a',
      'channel-b',
    ])
  })
})

describe('AdminChannelProvider — setChannelId', () => {
  it('updates channelId when setChannelId is called', async () => {
    server.use(channelsHandler({ channels: [CHANNEL_A, CHANNEL_B] }))

    const { result } = renderHook(() => useAdminChannel(), { wrapper })

    await waitFor(() => {
      expect(result.current.channelId).toBe('channel-a')
    })

    act(() => {
      result.current.setChannelId('channel-b')
    })

    expect(result.current.channelId).toBe('channel-b')
  })
})

describe('AdminChannelProvider — empty channels list', () => {
  it('keeps channelId null when the channels list is empty', async () => {
    server.use(channelsHandler({ channels: [] }))

    const { result } = renderHook(() => useAdminChannel(), { wrapper })

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(result.current.channelId).toBeNull()
    expect(result.current.channels).toEqual([])
    // Empty channels is an anomaly state — no error string, just a null selection
    // that consumers use to disable operations.
    expect(result.current.error).toBeNull()
  })
})

describe('AdminChannelProvider — failure and manual retry', () => {
  // The provider auto-retries on failure up to MAX_ATTEMPTS (3) with a real
  // `setTimeout` delay (5s). Waiting through 3 real intervals would blow the
  // test timeout, so this suite uses fake timers and advances them to fire the
  // auto-retry timers deterministically. `advanceTimersByTimeAsync` also flushes
  // the pending microtasks (the SDK fetch resolves) so MSW sees each attempt.
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  })

  it('sets error + null channelId after exhausting auto-retries, then retry recovers', async () => {
    // Persistently failing handler: every attempt returns 500.
    server.use(channelsHandler({ code: 500, message: 'boom' }, 500))

    const { result } = renderHook(() => useAdminChannel(), { wrapper })

    // Drive the auto-retry loop to completion. Initial mount fetch is attempt 1;
    // each subsequent advance fires one 5s retry timer. After MAX_ATTEMPTS the
    // provider gives up and surfaces `error`.
    // attempt 1 (mount) + attempts 2, 3 (retries) = 3 calls total before error.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000)
    })
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000)
    })

    // Three attempts fired, terminal error reached.
    expect(channelsCallCount).toBe(3)
    expect(result.current.error).not.toBeNull()
    expect(result.current.channelId).toBeNull()
    expect(result.current.loading).toBe(false)
    const failingCallCount = channelsCallCount

    // Restore real timers so the recovery path's fetch resolves via the normal
    // microtask queue (no further auto-retry timers are scheduled on success).
    vi.useRealTimers()

    // Swap the handler to a successful response BEFORE retrying, so the retry
    // request observes the 200.
    server.use(channelsHandler({ channels: [CHANNEL_A, CHANNEL_B] }))

    await act(async () => {
      result.current.retry()
    })

    // retry() must issue a fresh /api/channels request.
    await waitFor(() => {
      expect(channelsCallCount).toBeGreaterThan(failingCallCount)
    })

    // After the successful retry the provider selects the first channel and
    // clears the error.
    await waitFor(() => {
      expect(result.current.channelId).toBe('channel-a')
    })
    expect(result.current.error).toBeNull()
    expect(result.current.loading).toBe(false)
  })
})
