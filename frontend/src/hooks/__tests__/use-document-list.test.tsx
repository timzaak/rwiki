import { describe, it, expect, beforeEach } from 'vitest'
import { renderHook, waitFor, act } from '@testing-library/react'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { useDocumentList } from '@/hooks/use-document-list'
import type { DocumentListItem } from '@/lib/api-generated/types.gen'

/**
 * FE-T05 — useDocumentList hook tests
 *
 * Covers FE-D06's `useDocumentList()` (`src/hooks/use-document-list.ts`):
 *  - on mount calls `listDocuments` (GET /api/documents) once
 *  - success → `documents` populated from `result.data.documents`,
 *    `loading` false, `error` null
 *  - failure (non-2xx / network) → `error` set to `'加载失败'`, `loading` false
 *  - `refreshList()` re-fetches (the hook calls it on mount via useEffect)
 *
 * Strategy mirrors `use-feedback.test.tsx`: `renderHook` + MSW +
 * `client.setConfig({ baseUrl })` so the generated SDK's fetch reaches MSW.
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
  }
}

let listCallCount: number

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  localStorage.clear()
  listCallCount = 0
})

function installCountingHandler(docs: DocumentListItem[]) {
  server.use(
    http.get(LIST_URL, () => {
      listCallCount += 1
      return HttpResponse.json({ documents: docs })
    }),
  )
}

describe('useDocumentList — initial load', () => {
  it('loads documents on mount via listDocuments and clears loading', async () => {
    installCountingHandler([makeDoc('a', 'published'), makeDoc('b', 'draft')])

    const { result } = renderHook(() => useDocumentList())

    await waitFor(() => {
      expect(result.current.documents).toHaveLength(2)
    })

    expect(result.current.loading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(listCallCount).toBe(1)
    // The populated documents match the server response order/shape.
    expect(result.current.documents.map((d) => d.id)).toEqual(['a', 'b'])
  })

  it('starts with loading true and empty documents before fetch resolves', () => {
    // Synchronous assertion before any await — the hook's initial state.
    installCountingHandler([])

    const { result } = renderHook(() => useDocumentList())

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
    'sets error to 加载失败 and keeps documents empty on $label',
    async ({ status, body }) => {
      server.use(
        http.get(LIST_URL, () => HttpResponse.json(body, { status })),
      )

      const { result } = renderHook(() => useDocumentList())

      await waitFor(() => {
        expect(result.current.error).toBe('加载失败')
      })

      expect(result.current.loading).toBe(false)
      expect(result.current.documents).toEqual([])
    },
  )

  it('sets error on a network failure (fetch rejects)', async () => {
    server.use(http.get(LIST_URL, () => HttpResponse.error()))

    const { result } = renderHook(() => useDocumentList())

    await waitFor(() => {
      expect(result.current.error).toBe('加载失败')
    })
    expect(result.current.loading).toBe(false)
  })
})

describe('useDocumentList — refreshList', () => {
  it('refreshList re-fetches documents and updates the populated list', async () => {
    // First response: 2 docs.
    installCountingHandler([makeDoc('a'), makeDoc('b')])

    const { result } = renderHook(() => useDocumentList())

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

    const { result } = renderHook(() => useDocumentList())

    await waitFor(() => {
      expect(result.current.error).toBe('加载失败')
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
