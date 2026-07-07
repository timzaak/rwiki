import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { useDocumentList } from '@/hooks/use-document-list'
import type { DocumentListItem } from '@/lib/api-generated/types.gen'

/**
 * FE-T05 / FE-T03 — useDocumentList hook tests
 *
 * Covers FE-D06/FE-D03's `useDocumentList(channelId)` (`src/hooks/use-document-list.ts`):
 *  - on mount calls `listDocuments` (GET /api/documents) once, carrying
 *    `channelId` in the URL query (FE-D03 contract).
 *  - success → `documents` populated from `result.data.documents`,
 *    `loading` false, `error` null
 *  - failure (non-2xx / network) → `error` set to `'Failed to load'`, `loading` false
 *  - `refreshList()` re-fetches (the hook calls it on mount via useEffect)
 *  - `channelId === null` → skips the fetch entirely (zero requests).
 *  - switching `channelId` ('channel-a' → 'channel-b') re-fetches with the new query.
 *
 * Strategy mirrors `use-feedback.test.tsx`: `renderHook` + MSW +
 * `client.setConfig({ baseUrl })` so the generated SDK's fetch reaches MSW.
 * The query channelId is observed via the MSW handler's `request.url.searchParams`
 * — `listDocuments` is NOT mocked.
 */

const BASE_URL = 'http://localhost:3000'
const LIST_URL = `${BASE_URL}/api/documents`

function makeDoc(
  id: string,
  status: DocumentListItem['status'] = 'draft',
): DocumentListItem {
  return {
    id,
    fileName: `doc-${id}.pdf`,
    status,
    rowCount: 3,
    createdAt: '2026-01-01T00:00:00.000Z',
    errorMessage: null,
    channelId: 'channel-a',
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

function installCountingHandler(docs: DocumentListItem[]) {
  server.use(
    http.get(LIST_URL, ({ request }) => {
      listCallCount += 1
      lastSearchParams = new URL(request.url).searchParams
      return HttpResponse.json({ documents: docs })
    }),
  )
}

describe('useDocumentList — initial load', () => {
  it('loads documents on mount via listDocuments and clears loading', async () => {
    installCountingHandler([makeDoc('a', 'published'), makeDoc('b', 'draft')])

    const { result } = renderHook(() => useDocumentList('channel-a'))

    await waitFor(() => {
      expect(result.current.documents).toHaveLength(2)
    })

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(listCallCount).toBe(1)
    // FE-T03: channelId travels in the URL query.
    expect(lastSearchParams.get('channelId')).toBe('channel-a')
    // The populated documents match the server response order/shape.
    expect(result.current.documents.map((d) => d.id)).toEqual(['a', 'b'])
  })

  it('starts with loading true and empty documents before fetch resolves', () => {
    // Synchronous assertion before any await — the hook's initial state.
    installCountingHandler([])

    const { result } = renderHook(() => useDocumentList('channel-a'))

    expect(result.current.loading).toBe(true)
    expect(result.current.documents).toEqual([])
    expect(result.current.error).toBeNull()
  })
})

describe('useDocumentList — failure path', () => {
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
    'sets error to Failed to load and keeps documents empty on $label',
    async ({ status, body }) => {
      server.use(
        http.get(LIST_URL, () => HttpResponse.json(body, { status })),
      )

      const { result } = renderHook(() => useDocumentList('channel-a'))

      await waitFor(() => {
        expect(result.current.error).toBe('Failed to load')
      })

      expect(result.current.loading).toBe(false)
      expect(result.current.documents).toEqual([])
    },
  )

  it('sets error on a network failure (fetch rejects)', async () => {
    server.use(http.get(LIST_URL, () => HttpResponse.error()))

    const { result } = renderHook(() => useDocumentList('channel-a'))

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load')
    })
    expect(result.current.loading).toBe(false)
  })
})

describe('useDocumentList — refreshList', () => {
  it('refreshList re-fetches documents and updates the populated list', async () => {
    // First response: 2 docs.
    installCountingHandler([makeDoc('a'), makeDoc('b')])

    const { result } = renderHook(() => useDocumentList('channel-a'))

    await waitFor(() => {
      expect(result.current.documents).toHaveLength(2)
    })
    expect(listCallCount).toBe(1)

    // Swap the handler to return 3 docs, then re-fetch.
    server.use(
      http.get(LIST_URL, () => {
        listCallCount += 1
        return HttpResponse.json({
          documents: [makeDoc('a'), makeDoc('b'), makeDoc('c')],
        })
      }),
    )

    await act(async () => {
      await result.current.refreshList()
    })

    expect(result.current.documents).toHaveLength(3)
    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    // One mount fetch + one explicit refresh.
    expect(listCallCount).toBe(2)
  })

  it('clears a previous error after a successful refresh', async () => {
    // Initial fetch fails.
    server.use(
      http.get(LIST_URL, () =>
        HttpResponse.json({ code: 500, message: 'boom' }, { status: 500 }),
      ),
    )

    const { result } = renderHook(() => useDocumentList('channel-a'))

    await waitFor(() => {
      expect(result.current.error).toBe('Failed to load')
    })

    // Subsequent fetch succeeds.
    installCountingHandler([makeDoc('a')])

    await act(async () => {
      await result.current.refreshList()
    })

    expect(result.current.error).toBeNull()
    expect(result.current.documents).toHaveLength(1)
  })
})

describe('useDocumentList — channelId contract (FE-D03)', () => {
  it('skips the fetch and empties the list when channelId is null', async () => {
    installCountingHandler([makeDoc('a')])

    const { result } = renderHook(() => useDocumentList(null))

    // Give the hook a chance to run any effects before asserting zero requests.
    await waitFor(() => {
      expect(result.current.loading).toBe(false)
    })

    // channelId null → no request to the required-channelId endpoint.
    expect(listCallCount).toBe(0)
    expect(result.current.documents).toEqual([])
    expect(result.current.error).toBeNull()
  })

  it('re-fetches with the new channelId when channelId switches', async () => {
    installCountingHandler([makeDoc('a')])

    // Start on channel-a; the mount fetch carries channelId=channel-a.
    let channel: string | null = 'channel-a'
    const { result, rerender } = renderHook(() => useDocumentList(channel))

    await waitFor(() => {
      expect(listCallCount).toBe(1)
    })
    expect(lastSearchParams.get('channelId')).toBe('channel-a')

    // Switch to channel-b → effect re-runs (dep array includes channelId) and a
    // second request carries channelId=channel-b.
    channel = 'channel-b'
    rerender()

    await waitFor(() => {
      expect(listCallCount).toBe(2)
    })
    expect(lastSearchParams.get('channelId')).toBe('channel-b')
    expect(result.current.error).toBeNull()
  })
})
