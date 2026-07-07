import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import { type ReactNode } from 'react'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import {
  AdminSiteProvider,
  useAdminSite,
} from '@/lib/admin-site-context'

/**
 * FE-T03 — AdminSiteProvider global site context tests
 *
 * Covers FE-D03's `admin-site-context.tsx`:
 *  - default-first: on success with a non-empty list, `siteId` becomes the first
 *    site's id (`sites[0].id`), `loading===false`, `error===null`, `sites`
 *    populated.
 *  - switch: `setSiteId('site-b')` updates `siteId`.
 *  - empty: success with `sites: []` → `siteId===null` (consumers disable).
 *  - fail + retry: 500 → `error` non-null, `siteId===null`; `retry()` re-requests
 *    `/api/sites`; swapping to a 200 handler → after retry `siteId==='site-a'`,
 *    `error===null`.
 *
 * Strategy mirrors `use-feedback.test.tsx` / `api-client-setup.test.tsx`:
 * `renderHook(... , { wrapper })` + MSW `/api/sites` + `client.setConfig({
 * baseUrl })` so the generated SDK's fetch reaches MSW. `listSites` is NOT
 * mocked — the real SDK fetch flows through MSW.
 *
 * We do NOT assert the exact localStorage key (an implementation detail). The
 * provider auto-retries on failure with a delay; tests drive recovery via the
 * manual `retry()` (which cancels pending timers and re-loads immediately) and
 * only assert the externally observable contract.
 */

const BASE_URL = 'http://localhost:3000'
const SITES_URL = `${BASE_URL}/api/sites`

const SITE_A = { id: 'site-a', name: 'A' }
const SITE_B = { id: 'site-b', name: 'B' }

// renderHook wrapper: AdminSiteProvider owns listSites + the context value.
function wrapper({ children }: { children: ReactNode }) {
  return <AdminSiteProvider>{children}</AdminSiteProvider>
}

let sitesCallCount: number

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  localStorage.clear()
  sitesCallCount = 0
})

afterEach(() => {
  // Ensure fake timers (used by the failure/retry suite) never leak into the
  // other suites, which rely on real timers + waitFor.
  vi.useRealTimers()
})

function sitesHandler(
  body: Record<string, unknown>,
  status: 200 | 500 = 200,
): ReturnType<typeof http.get> {
  return http.get(SITES_URL, () => {
    sitesCallCount += 1
    if (status === 500) {
      return HttpResponse.json(
        { code: 500, message: 'boom' },
        { status: 500 },
      )
    }
    return HttpResponse.json(body)
  })
}

describe('AdminSiteProvider — default-first selection', () => {
  it('defaults siteId to the first site and clears loading on success', async () => {
    server.use(sitesHandler({ sites: [SITE_A, SITE_B] }))

    const { result } = renderHook(() => useAdminSite(), { wrapper })

    await waitFor(() => {
      expect(result.current.siteId).toBe('site-a')
    })

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(result.current.sites).toHaveLength(2)
    expect(result.current.sites.map((s) => s.id)).toEqual([
      'site-a',
      'site-b',
    ])
  })
})

describe('AdminSiteProvider — setSiteId', () => {
  it('updates siteId when setSiteId is called', async () => {
    server.use(sitesHandler({ sites: [SITE_A, SITE_B] }))

    const { result } = renderHook(() => useAdminSite(), { wrapper })

    await waitFor(() => {
      expect(result.current.siteId).toBe('site-a')
    })

    act(() => {
      result.current.setSiteId('site-b')
    })

    expect(result.current.siteId).toBe('site-b')
  })
})

describe('AdminSiteProvider — empty sites list', () => {
  it('keeps siteId null when the sites list is empty', async () => {
    server.use(sitesHandler({ sites: [] }))

    const { result } = renderHook(() => useAdminSite(), { wrapper })

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(result.current.siteId).toBeNull()
    expect(result.current.sites).toEqual([])
    // Empty sites is an anomaly state — no error string, just a null selection
    // that consumers use to disable operations.
    expect(result.current.error).toBeNull()
  })
})

describe('AdminSiteProvider — failure and manual retry', () => {
  // The provider auto-retries on failure up to MAX_ATTEMPTS (3) with a real
  // `setTimeout` delay (5s). Waiting through 3 real intervals would blow the
  // test timeout, so this suite uses fake timers and advances them to fire the
  // auto-retry timers deterministically. `advanceTimersByTimeAsync` also flushes
  // the pending microtasks (the SDK fetch resolves) so MSW sees each attempt.
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  })

  it('sets error + null siteId after exhausting auto-retries, then retry recovers', async () => {
    // Persistently failing handler: every attempt returns 500.
    server.use(sitesHandler({ code: 500, message: 'boom' }, 500))

    const { result } = renderHook(() => useAdminSite(), { wrapper })

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
    expect(sitesCallCount).toBe(3)
    expect(result.current.error).not.toBeNull()
    expect(result.current.siteId).toBeNull()
    expect(result.current.loading).toBe(false)
    const failingCallCount = sitesCallCount

    // Restore real timers so the recovery path's fetch resolves via the normal
    // microtask queue (no further auto-retry timers are scheduled on success).
    vi.useRealTimers()

    // Swap the handler to a successful response BEFORE retrying, so the retry
    // request observes the 200.
    server.use(sitesHandler({ sites: [SITE_A, SITE_B] }))

    await act(async () => {
      result.current.retry()
    })

    // retry() must issue a fresh /api/sites request.
    await waitFor(() => {
      expect(sitesCallCount).toBeGreaterThan(failingCallCount)
    })

    // After the successful retry the provider selects the first site and
    // clears the error.
    await waitFor(() => {
      expect(result.current.siteId).toBe('site-a')
    })
    expect(result.current.error).toBeNull()
    expect(result.current.loading).toBe(false)
  })
})
