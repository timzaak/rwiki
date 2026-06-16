import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { BatchActions } from '@/components/admin/batch-actions'
import type {
  BatchStatusItem,
  DocumentListItem,
} from '@/lib/api-generated/types.gen'

/**
 * FE-T04 — BatchActions CORE regression tests
 *
 * Guards the design §4.1 / §4.4.3 invariant: a single batch publish or
 * unpublish action MUST trigger exactly ONE `POST /api/documents/batch-status`
 * request carrying a combined `{ publish, unpublish }` body — never per-document
 * `publishDocument` / `unpublishDocument` calls.
 *
 * Strategy: MSW intercepts the real network path the generated SDK emits
 * (`client.post('/api/documents/batch-status')` after `setConfig({ baseUrl })`),
 * a closure-captured array records each request body, and tests assert on the
 * array length (exactly one) and shape. No internal SDK function is mocked.
 */

const BASE_URL = 'http://localhost:3000'

type BatchStatusBody = { publish: string[]; unpublish: string[] }

let batchStatusRequests: BatchStatusBody[]
let deleteRequests: string[]
let singlePublishHit: boolean
let singleUnpublishHit: boolean

function makeDoc(
  id: string,
  status: DocumentListItem['status'] = 'draft',
): DocumentListItem {
  return {
    id,
    fileName: `doc-${id}.pdf`,
    status,
    rowCount: 5,
    createdAt: '2026-01-01',
    errorMessage: null,
  }
}

function makeResult(
  overrides: Partial<BatchStatusItem>,
): BatchStatusItem {
  return {
    documentId: 'a',
    action: 'publish',
    applied: true,
    status: 'published',
    reason: null,
    ...overrides,
  }
}

interface RenderOpts {
  selectedIds?: Set<string>
  documents?: DocumentListItem[]
  onCompleted?: () => void
}

function renderBatchActions(opts: RenderOpts = {}) {
  const onCompleted = opts.onCompleted ?? vi.fn()
  const utils = render(
    <BatchActions
      selectedIds={opts.selectedIds ?? new Set()}
      documents={opts.documents ?? []}
      onCompleted={onCompleted}
    />,
  )
  return { ...utils, onCompleted }
}

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  batchStatusRequests = []
  deleteRequests = []
  singlePublishHit = false
  singleUnpublishHit = false
  vi.clearAllMocks()
})

// Default MSW handlers: record every batch-status body, every delete id, and
// flag any forbidden single-document publish/unpublish call on the batch path.
// Per-test `server.use()` overrides are reset automatically by setup.ts
// `afterEach(() => server.resetHandlers())`.
function installCountingHandlers(results: BatchStatusItem[] = []) {
  server.use(
    http.post(`${BASE_URL}/api/documents/batch-status`, async ({ request }) => {
      batchStatusRequests.push((await request.json()) as BatchStatusBody)
      return HttpResponse.json({ results }, { status: 200 })
    }),
    http.delete(`${BASE_URL}/api/documents/:id`, ({ params }) => {
      deleteRequests.push(params.id as string)
      return new HttpResponse(null, { status: 204 })
    }),
    // Forbidden single-document endpoints — the batch path must never hit these.
    http.patch(`${BASE_URL}/api/documents/:id/publish`, () => {
      singlePublishHit = true
      return new HttpResponse(null, { status: 204 })
    }),
    http.patch(`${BASE_URL}/api/documents/:id/unpublish`, () => {
      singleUnpublishHit = true
      return new HttpResponse(null, { status: 204 })
    }),
  )
}

describe('BatchActions — CORE: exactly one batch-status request', () => {
  it.each([
    {
      action: 'publish' as const,
      button: 'batch-publish-button',
      status: 'draft' as const,
      expectedField: 'publish' as const,
      otherField: 'unpublish' as const,
      resultStatus: 'published',
    },
    {
      action: 'unpublish' as const,
      button: 'batch-unpublish-button',
      status: 'published' as const,
      expectedField: 'unpublish' as const,
      otherField: 'publish' as const,
      resultStatus: 'draft',
    },
  ])(
    'batch $action submits exactly ONE batch-status with $expectedField=[ids] and $otherField=[]',
    async ({ button, status, expectedField, otherField, action, resultStatus }) => {
      installCountingHandlers([
        makeResult({ documentId: 'a', action, status: resultStatus }),
        makeResult({ documentId: 'b', action, status: resultStatus }),
      ])

      const documents = [makeDoc('a', status), makeDoc('b', status)]
      const user = userEvent.setup()
      renderBatchActions({
        selectedIds: new Set(['a', 'b']),
        documents,
      })

      await user.click(screen.getByTestId(button))

      // CORE ASSERTION — exactly one POST /api/documents/batch-status.
      await waitFor(() => expect(batchStatusRequests).toHaveLength(1))
      expect(batchStatusRequests).toHaveLength(1)

      const body = batchStatusRequests[0]
      expect(body[expectedField]).toEqual(['a', 'b'])
      expect(body[otherField]).toEqual([])

      // No single-document publish/unpublish leaked onto the batch path.
      expect(singlePublishHit).toBe(false)
      expect(singleUnpublishHit).toBe(false)
    },
  )

  it('short-circuits with zero requests when no selected doc matches the action', async () => {
    installCountingHandlers([])

    // Two published docs selected, but we click PUBLISH (only drafts qualify).
    const documents = [makeDoc('a', 'published'), makeDoc('b', 'published')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a', 'b']),
      documents,
    })

    await user.click(screen.getByTestId('batch-publish-button'))

    // No matching draft → must not issue any request.
    await waitFor(() => {
      expect(batchStatusRequests).toHaveLength(0)
    })
    expect(batchStatusRequests).toHaveLength(0)
  })
})

describe('BatchActions — per-item feedback', () => {
  it('renders per-item feedback rows from results[] (applied + reason)', async () => {
    installCountingHandlers([
      makeResult({ documentId: 'a', applied: true, status: 'published', reason: null }),
      makeResult({ documentId: 'b', applied: false, status: 'draft', reason: 'invalid_status' }),
    ])

    const documents = [makeDoc('a', 'draft'), makeDoc('b', 'draft')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a', 'b']),
      documents,
    })

    await user.click(screen.getByTestId('batch-publish-button'))

    const items = await screen.findAllByTestId('batch-feedback-item')
    expect(items).toHaveLength(2)
    // First row: applied
    expect(items[0]).toHaveTextContent('doc-a.pdf')
    expect(items[0]).toHaveTextContent('已生效')
    // Second row: not applied + reason label
    expect(items[1]).toHaveTextContent('doc-b.pdf')
    expect(items[1]).toHaveTextContent('未生效')
    expect(items[1]).toHaveTextContent('状态不允许')
  })
})

describe('BatchActions — batch delete', () => {
  it('calls deleteDocument once per id and never touches batch-status', async () => {
    installCountingHandlers([])

    const documents = [
      makeDoc('a', 'draft'),
      makeDoc('b', 'published'),
      makeDoc('c', 'draft'),
    ]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a', 'b', 'c']),
      documents,
    })

    await user.click(screen.getByTestId('batch-delete-button'))

    await waitFor(() => expect(deleteRequests).toHaveLength(3))
    expect(deleteRequests).toHaveLength(3)
    expect(deleteRequests).toEqual(expect.arrayContaining(['a', 'b', 'c']))
    // Delete path must not invoke the batch-status endpoint.
    expect(batchStatusRequests).toHaveLength(0)
  })

  it('only deletes selected docs that are in the passed documents (data-loss guard)', async () => {
    installCountingHandlers([])

    // 'c' is selected but NOT in documents (e.g. hidden by a status filter).
    // REGRESSION GUARD: delete must scope to documents ∩ selectedIds; it must
    // never delete an id absent from the visible set.
    const documents = [makeDoc('a', 'draft'), makeDoc('b', 'published')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a', 'b', 'c']),
      documents,
    })

    await user.click(screen.getByTestId('batch-delete-button'))

    await waitFor(() => expect(deleteRequests).toHaveLength(2))
    expect(deleteRequests).toEqual(expect.arrayContaining(['a', 'b']))
    expect(deleteRequests).not.toContain('c')
  })
})

describe('BatchActions — disabled state', () => {
  it('disables all three buttons when selectedIds is empty', () => {
    renderBatchActions({ selectedIds: new Set(), documents: [] })

    expect(screen.getByTestId('batch-publish-button')).toBeDisabled()
    expect(screen.getByTestId('batch-unpublish-button')).toBeDisabled()
    expect(screen.getByTestId('batch-delete-button')).toBeDisabled()
  })

  it('disables all three buttons while a batch submit is in flight', async () => {
    let resolveResponse: () => void
    const responsePromise = new Promise<void>(
      (resolve) => {
        resolveResponse = resolve
      },
    )

    server.use(
      http.post(`${BASE_URL}/api/documents/batch-status`, async () => {
        await responsePromise
        return HttpResponse.json({ results: [] }, { status: 200 })
      }),
    )

    const documents = [makeDoc('a', 'draft')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a']),
      documents,
    })

    // Click and immediately assert disabled before the response resolves.
    await user.click(screen.getByTestId('batch-publish-button'))
    await waitFor(() => {
      expect(screen.getByTestId('batch-publish-button')).toBeDisabled()
    })
    expect(screen.getByTestId('batch-unpublish-button')).toBeDisabled()
    expect(screen.getByTestId('batch-delete-button')).toBeDisabled()

    resolveResponse!()

    // Buttons re-enable after completion.
    await waitFor(() => {
      expect(screen.getByTestId('batch-publish-button')).toBeEnabled()
    })
  })
})

describe('BatchActions — onCompleted', () => {
  it('calls onCompleted once after a successful batch publish', async () => {
    installCountingHandlers([
      makeResult({ documentId: 'a', action: 'publish', applied: true, status: 'published' }),
    ])

    const onCompleted = vi.fn()
    const documents = [makeDoc('a', 'draft')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a']),
      documents,
      onCompleted,
    })

    await user.click(screen.getByTestId('batch-publish-button'))

    await waitFor(() => expect(onCompleted).toHaveBeenCalledTimes(1))
  })

  it('calls onCompleted once after a successful batch delete', async () => {
    installCountingHandlers([])

    const onCompleted = vi.fn()
    const documents = [makeDoc('a', 'draft')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a']),
      documents,
      onCompleted,
    })

    await user.click(screen.getByTestId('batch-delete-button'))

    await waitFor(() => expect(onCompleted).toHaveBeenCalledTimes(1))
  })
})
