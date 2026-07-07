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
import {
  seedSite,
  seedEmptySites,
  withAdminSiteProvider,
} from '@/test/helpers/admin-site'
import { useAdminSite } from '@/lib/admin-site-context'

/**
 * FE-T04 / FE-T03 — BatchActions CORE regression + site-context tests
 *
 * Guards the design §4.1 / §4.4.3 invariant: a single batch publish or
 * unpublish action MUST trigger exactly ONE `POST /api/documents/batch-status`
 * request carrying a combined `{ publish, unpublish }` body — never per-document
 * `publishDocument` / `unpublishDocument` calls.
 *
 * FE-T03 extends this with the global site-context contract (FE-D03):
 *  - BatchActions consumes `useAdminSite()`; when wrapped in AdminSiteProvider
 *    seeded with `site-a`, the batch-status request body carries
 *    `siteId === 'site-a'` (siteId in BODY — BatchStatusRequest.siteId).
 *  - the per-id delete request carries `siteId === 'site-a'` in the URL QUERY
 *    (`DELETE /api/documents/:id?siteId=`), NOT in the body.
 *  - when `siteId === null` (empty-sites anomaly), all three buttons are
 *    disabled even with selectedIds present.
 *
 * The batch body.siteId vs delete query.siteId location split is load-bearing:
 * do NOT assert delete's siteId in body or batch's siteId in query.
 *
 * Strategy: MSW intercepts the real network path the generated SDK emits
 * (`client.post('/api/documents/batch-status')` after `setConfig({ baseUrl })`),
 * a closure-captured array records each request body, and tests assert on the
 * array length (exactly one) and shape. No internal SDK function is mocked.
 * The provider is seeded via MSW `/api/sites` (see `helpers/admin-site`); the
 * prod context is private, so injection goes through the real provider.
 */

const BASE_URL = 'http://localhost:3000'

type BatchStatusBody = {
  publish: string[]
  unpublish: string[]
  siteId: string
}

let batchStatusRequests: BatchStatusBody[]
let deleteRequests: string[]
let deleteSiteIds: (string | null)[]
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
    siteId: 'site-a',
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

// Probe rendered inside the provider to expose when bootstrap has settled
// (loading false). The provider's listSites() runs async on mount; until it
// resolves, siteId is null and the batch buttons are disabled, so interaction
// tests must wait for readiness before clicking. `waitForSiteReady` awaits
// this sentinel before each interaction.
function SiteReadyProbe() {
  const { loading } = useAdminSite()
  return (
    <span
      data-testid="site-ready-probe"
      data-loading={loading ? 'true' : 'false'}
    />
  )
}

function renderBatchActions(opts: RenderOpts = {}) {
  const onCompleted = opts.onCompleted ?? vi.fn()
  const utils = render(
    withAdminSiteProvider({
      children: (
        <>
          <SiteReadyProbe />
          <BatchActions
            selectedIds={opts.selectedIds ?? new Set()}
            documents={opts.documents ?? []}
            onCompleted={onCompleted}
          />
        </>
      ),
    }),
  )
  return { ...utils, onCompleted }
}

// Await the provider's listSites() bootstrap so the BatchActions buttons reach
// their settled (site-selected / empty) state before interactions run.
async function waitForSiteReady() {
  await waitFor(() => {
    expect(
      screen.getByTestId('site-ready-probe').getAttribute('data-loading'),
    ).toBe('false')
  })
}

beforeEach(() => {
  // jsdom requires an absolute URL for the SDK's fetch to reach MSW.
  client.setConfig({ baseUrl: BASE_URL })
  // Default seed: a single site so the provider auto-selects siteId='site-a'.
  // Cases that need a different/null siteId override via seedEmptySites().
  seedSite('site-a')
  batchStatusRequests = []
  deleteRequests = []
  deleteSiteIds = []
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
    http.delete(`${BASE_URL}/api/documents/:id`, ({ params, request }) => {
      deleteRequests.push(params.id as string)
      // FE-T03: delete's siteId lives in the URL QUERY (not the body).
      deleteSiteIds.push(
        new URL(request.url).searchParams.get('siteId'),
      )
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
      await waitForSiteReady()

      await user.click(screen.getByTestId(button))

      // CORE ASSERTION — exactly one POST /api/documents/batch-status.
      await waitFor(() => expect(batchStatusRequests).toHaveLength(1))
      expect(batchStatusRequests).toHaveLength(1)

      const body = batchStatusRequests[0]
      expect(body[expectedField]).toEqual(['a', 'b'])
      expect(body[otherField]).toEqual([])

      // FE-T03: siteId travels in the BODY (BatchStatusRequest.siteId), equal
      // to the injected provider siteId. Load-bearing: body, NOT query.
      expect(body.siteId).toBe('site-a')

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
    await waitForSiteReady()

    await user.click(screen.getByTestId('batch-publish-button'))

    // No matching draft → must not issue any request.
    await waitFor(() => {
      expect(batchStatusRequests).toHaveLength(0)
    })
    expect(batchStatusRequests).toHaveLength(0)
  })
})

describe('BatchActions — concise feedback (summary + only failures)', () => {
  it('summarizes all-applied results with one line and NO per-item rows', async () => {
    installCountingHandlers([
      makeResult({ documentId: 'a', applied: true, status: 'published', reason: null }),
      makeResult({ documentId: 'b', applied: true, status: 'published', reason: null }),
    ])

    const documents = [makeDoc('a', 'draft'), makeDoc('b', 'draft')]
    const user = userEvent.setup()
    renderBatchActions({
      selectedIds: new Set(['a', 'b']),
      documents,
    })
    await waitForSiteReady()

    await user.click(screen.getByTestId('batch-publish-button'))

    // One summary line; success noise is removed entirely.
    const feedback = await screen.findByTestId('batch-feedback')
    expect(feedback).toHaveTextContent('Published 2 documents.')
    expect(screen.queryAllByTestId('batch-feedback-item')).toHaveLength(0)
  })

  it('enumerates ONLY failed items (name + reason) when some did not apply', async () => {
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
    await waitForSiteReady()

    await user.click(screen.getByTestId('batch-publish-button'))

    const feedback = await screen.findByTestId('batch-feedback')
    expect(feedback).toHaveTextContent('Published 1 of 2 documents.')

    // Only the failure is listed; the applied doc is NOT.
    const items = screen.getAllByTestId('batch-feedback-item')
    expect(items).toHaveLength(1)
    expect(items[0]).toHaveTextContent('doc-b.pdf')
    expect(items[0]).toHaveTextContent('Not applied')
    expect(items[0]).toHaveTextContent('Invalid status')
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
    await waitForSiteReady()

    await user.click(screen.getByTestId('batch-delete-button'))

    await waitFor(() => expect(deleteRequests).toHaveLength(3))
    expect(deleteRequests).toHaveLength(3)
    expect(deleteRequests).toEqual(expect.arrayContaining(['a', 'b', 'c']))
    // Delete path must not invoke the batch-status endpoint.
    expect(batchStatusRequests).toHaveLength(0)
    // FE-T03: every per-id delete carries siteId in the URL QUERY (not body).
    // Load-bearing: query, NOT body.
    expect(deleteSiteIds).toEqual(['site-a', 'site-a', 'site-a'])
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
    await waitForSiteReady()

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
    await waitForSiteReady()

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
    await waitForSiteReady()

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
    await waitForSiteReady()

    await user.click(screen.getByTestId('batch-delete-button'))

    await waitFor(() => expect(onCompleted).toHaveBeenCalledTimes(1))
  })
})

describe('BatchActions — null siteId disables all operations (FE-D03)', () => {
  it('disables publish/unpublish/delete even with selectedIds when siteId is null', async () => {
    // Empty-sites anomaly → provider keeps siteId === null.
    seedEmptySites()

    const documents = [makeDoc('a', 'draft'), makeDoc('b', 'published')]
    renderBatchActions({
      selectedIds: new Set(['a', 'b']),
      documents,
    })
    await waitForSiteReady()

    // All three buttons disabled despite a non-empty selection: siteId missing
    // is a hard gate (the operations cannot be dispatched without it).
    expect(screen.getByTestId('batch-publish-button')).toBeDisabled()
    expect(screen.getByTestId('batch-unpublish-button')).toBeDisabled()
    expect(screen.getByTestId('batch-delete-button')).toBeDisabled()
    // No operation leaked.
    expect(batchStatusRequests).toHaveLength(0)
    expect(deleteRequests).toHaveLength(0)
  })
})
