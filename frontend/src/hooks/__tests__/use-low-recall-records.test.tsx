import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { useLowRecallRecords } from '@/hooks/use-low-recall-records'
import type {
  LowRecallRecord,
  LowRecallSource,
} from '@/lib/api-generated/types.gen'

/**
 * FE-T01 / FE-T03 — useLowRecallRecords hook tests
 *
 * Covers FE-D02/FE-D03's `useLowRecallRecords({ channelId, minScore?, maxScore?,
 * from?, to?, limit?, offset? })` (`src/hooks/use-low-recall-records.ts`):
 *  - on mount calls `listLowRecallRecords` (GET /api/low-recall/records) once,
 *    carrying the required `channelId` as the FIRST query param (FE-D03 contract).
 *  - success → `items`/`total` populated, `loading` false, `error` null
 *  - query params (minScore/maxScore/from/to) passed through and re-fetched
 *    on change; `null` params omitted (null → undefined)
 *  - pagination (limit/offset) re-fetches on change
 *  - empty result → `items=[]`, `total=0`
 *  - failure (non-2xx / network) → `error` set to `'Failed to load'`,
 *    `loading` false. 401 is NOT handled by the hook (global interceptor
 *    owns redirect / Key cleanup); this hook only sets `error='Failed to load'`.
 *  - `channelId === null` → skips the fetch entirely (zero requests).
 *
 * Strategy mirrors `use-document-list.test.tsx`: `renderHook` + MSW +
 * `client.setConfig({ baseUrl })` so the generated SDK's fetch reaches MSW.
 * Query params are observed via the MSW handler's `request.url.searchParams`
 * — `listLowRecallRecords` is NOT mocked.
 */

const BASE_URL = 'http://localhost:3000'
const LIST_URL = `${BASE_URL}/api/low-recall/records`

function makeSource(overrides?: Partial<LowRecallSource>): LowRecallSource {
  return {
    documentId: 'doc-1',
    chunkId: 'chunk-1',
    title: 'Source chunk',
    score: 0.123,
    ...overrides,
  }
}

function makeRecord(overrides?: Partial<LowRecallRecord>): LowRecallRecord {
  return {
    id: 1,
    query: 'some query',
    resultCount: 2,
    createdAt: '2026-01-15T00:00:00.000Z',
    sources: [],
    topScore: null,
    sessionId: null,
    channelId: 'channel-a',
    ...overrides,
  }
}

let listCallCount: number
let lastSearchParams: URLSearchParams

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  localStorage.clear()
  listCallCount = 0
  lastSearchParams = new URLSearchParams()
})

function installCountingHandler(
  items: LowRecallRecord[],
  total?: number,
): void {
  server.use(
    http.get(LIST_URL, ({ request }) => {
      listCallCount += 1
      lastSearchParams = new URL(request.url).searchParams
      return HttpResponse.json({
        items,
        total: total ?? items.length,
      })
    }),
  )
}

describe('useLowRecallRecords — initial load', () => {
  it('loads records on mount via listLowRecallRecords and clears loading', async () => {
    installCountingHandler([
      makeRecord({
        id: 1,
        query: 'first',
        resultCount: 0,
        topScore: null,
        sources: [],
      }),
      makeRecord({
        id: 2,
        query: 'second',
        resultCount: 3,
        topScore: 0.2,
        sources: [makeSource({ documentId: 'd2', chunkId: 'c2', score: 0.2 })],
      }),
    ])

    const { result } = renderHook(() => useLowRecallRecords({ channelId: 'channel-a' }))

    await waitFor(() => {
      expect(result.current.items).toHaveLength(2)
    })

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(listCallCount).toBe(1)
    // The populated items match the server response order/shape.
    expect(result.current.items.map((r) => r.id)).toEqual([1, 2])
    expect(result.current.total).toBe(2)
  })

  it('starts with loading true and empty items before fetch resolves', () => {
    // Synchronous assertion before any await — the hook's initial state.
    installCountingHandler([])

    const { result } = renderHook(() => useLowRecallRecords({ channelId: 'channel-a' }))

    expect(result.current.loading).toBe(true)
    expect(result.current.items).toEqual([])
    expect(result.current.total).toBe(0)
    expect(result.current.error).toBeNull()
  })
})

describe('useLowRecallRecords — query params', () => {
  it('passes minScore/maxScore/from/to as query params when provided', async () => {
    installCountingHandler([makeRecord()])

    const { result } = renderHook(() =>
      useLowRecallRecords({
        channelId: 'channel-a',
        minScore: 0.1,
        maxScore: 0.3,
        from: '2026-01-01T00:00:00.000Z',
        to: '2026-02-01T00:00:00.000Z',
      }),
    )

    await waitFor(() => {
      expect(result.current.items).toHaveLength(1)
    })

    expect(listCallCount).toBe(1)
    // FE-T03: channelId is carried alongside the other query params.
    expect(lastSearchParams.get('channelId')).toBe('channel-a')
    // Query params observed through the MSW handler (NOT via mocking the SDK).
    expect(lastSearchParams.get('minScore')).toBe('0.1')
    expect(lastSearchParams.get('maxScore')).toBe('0.3')
    expect(lastSearchParams.get('from')).toBe('2026-01-01T00:00:00.000Z')
    expect(lastSearchParams.get('to')).toBe('2026-02-01T00:00:00.000Z')
  })

  it('omits null params from the query (null -> undefined)', async () => {
    installCountingHandler([])

    const { result } = renderHook(() =>
      useLowRecallRecords({
        channelId: 'channel-a',
        minScore: null,
        maxScore: null,
        from: null,
        to: null,
      }),
    )

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(listCallCount).toBe(1)
    // null → undefined: these params must NOT appear in the query string.
    expect(lastSearchParams.has('minScore')).toBe(false)
    expect(lastSearchParams.has('maxScore')).toBe(false)
    expect(lastSearchParams.has('from')).toBe(false)
    expect(lastSearchParams.has('to')).toBe(false)
  })

  it('re-fetches when filter params change', async () => {
    installCountingHandler([makeRecord()])

    let filters: { channelId: string | null; minScore?: number } = {
      channelId: 'channel-a',
    }
    const { result, rerender } = renderHook(() =>
      useLowRecallRecords(filters),
    )

    // Mount fetch (no minScore filter) completes first.
    await waitFor(() => {
      expect(listCallCount).toBe(1)
    })
    expect(lastSearchParams.has('minScore')).toBe(false)

    // Drive a filter change → effect re-runs (dep array includes minScore).
    filters = { channelId: 'channel-a', minScore: 0.2 }
    rerender()

    await waitFor(() => {
      expect(listCallCount).toBe(2)
    })
    expect(lastSearchParams.get('minScore')).toBe('0.2')
    expect(result.current.error).toBeNull()
  })
})

describe('useLowRecallRecords — pagination', () => {
  it('passes limit/offset as query params and re-fetches on change', async () => {
    installCountingHandler([makeRecord()], 50)

    let pagination: { channelId: string | null; limit: number; offset: number } = {
      channelId: 'channel-a',
      limit: 10,
      offset: 0,
    }
    const { rerender } = renderHook(() =>
      useLowRecallRecords(pagination),
    )

    await waitFor(() => {
      expect(listCallCount).toBe(1)
    })
    expect(lastSearchParams.get('limit')).toBe('10')
    expect(lastSearchParams.get('offset')).toBe('0')

    // Advance to the next page → effect re-runs (dep array includes offset).
    pagination = { channelId: 'channel-a', limit: 10, offset: 10 }
    rerender()

    await waitFor(() => {
      expect(listCallCount).toBe(2)
    })
    expect(lastSearchParams.get('limit')).toBe('10')
    expect(lastSearchParams.get('offset')).toBe('10')
  })
})

describe('useLowRecallRecords — empty result', () => {
  it('populates items=[] and total=0 on empty result', async () => {
    server.use(
      http.get(LIST_URL, () => {
        listCallCount += 1
        return HttpResponse.json({ items: [], total: 0 })
      }),
    )

    const { result } = renderHook(() => useLowRecallRecords({ channelId: 'channel-a' }))

    // Gate on loading=false: total starts at 0 and items at [] (initial state),
    // so only the post-fetch loading flip proves the empty response was applied.
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    expect(result.current.items).toEqual([])
    expect(result.current.total).toBe(0)
    expect(result.current.error).toBeNull()
  })
})

describe('useLowRecallRecords — failure path', () => {
  it.each([
    {
      label: '500 Server Error',
      status: 500,
      body: { code: 500, message: 'boom' },
    },
    {
      label: '401 Unauthorized',
      status: 401,
      body: { code: 401, message: 'Unauthorized' },
    },
  ])(
    'sets error to Failed to load and keeps items empty on $label',
    async ({ status, body }) => {
      server.use(
        http.get(LIST_URL, () => HttpResponse.json(body, { status })),
      )

      const { result } = renderHook(() => useLowRecallRecords({ channelId: 'channel-a' }))

      await waitFor(() => {
        expect(result.current.error).toBe('Failed to load')
      })

      expect(result.current.loading).toBe(false)
      expect(result.current.items).toEqual([])
      // NOTE: 401 redirect / Key cleanup is the global interceptor's job —
      // out of this hook's scope. Only assert the hook sets error here.
    },
  )

  it('sets error on a network failure (fetch rejects)', async () => {
    server.use(http.get(LIST_URL, () => HttpResponse.error()))

    const { result } = renderHook(() => useLowRecallRecords({ channelId: 'channel-a' }))

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load')
    })
    expect(result.current.loading).toBe(false)
  })
})

describe('useLowRecallRecords — channelId contract (FE-D03)', () => {
  it('skips the fetch when channelId is null', async () => {
    installCountingHandler([makeRecord()])

    const { result } = renderHook(() =>
      useLowRecallRecords({ channelId: null }),
    )

    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    // channelId null → no request to the required-channelId endpoint.
    expect(listCallCount).toBe(0)
    expect(result.current.items).toEqual([])
    expect(result.current.total).toBe(0)
    expect(result.current.error).toBeNull()
  })
})
