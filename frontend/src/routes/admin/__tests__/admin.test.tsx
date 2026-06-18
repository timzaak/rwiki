import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { isRedirect } from '@tanstack/router-core'
import {
  createRootRoute,
  createRoute,
  createRouter,
  createMemoryHistory,
  RouterProvider,
  Outlet,
} from '@tanstack/react-router'
import type { ComponentType } from 'react'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import type { DocumentListItem } from '@/lib/api-generated/types.gen'
import { AdminLayout } from '@/components/admin/admin-layout'

/**
 * FE-T03 — admin multi-page route guard migration regression.
 *
 * After FE-D01 the guard moved from `routes/admin/index.tsx` to the layout
 * route `routes/admin/route.tsx` (`createFileRoute('/admin')`), whose
 * `component: AdminLayout` renders the page header + nav (`admin-nav` with
 * `<Link to="/admin">` + `<Link to="/admin/low-recall">`) + `<Outlet/>`.
 * `routes/admin/index.tsx` (`createFileRoute('/admin/')`) keeps the Document
 * Management body but NO LONGER owns the guard (it inherits from the parent).
 *
 * Because the guard and the page body now live in TWO DIFFERENT files, the old
 * single-import pattern (`const { Route } = await import('@/routes/admin')`
 * then reading BOTH `beforeLoad` AND `component`) is stale: depending on
 * module resolution `@/routes/admin` may resolve to either `route.tsx` or
 * `index.tsx`, mixing the guard with the wrong component. To stay unambiguous
 * under `moduleResolution: Bundler`, we split the imports explicitly:
 *   - `beforeLoad` from `@/routes/admin/route` (the layout route).
 *   - Document Management `component` from `@/routes/admin/index`.
 *   - `AdminLayout` (for the nav-visibility case) imported directly from
 *     `@/components/admin/admin-layout` to preserve its concrete component type
 *     for `createRoute({ component })` (the cast-to-`unknown` Route option drops
 *     the `RouteComponent` shape that `createRoute` expects).
 *
 * Coverage:
 *  - `beforeLoad` guard: `!isAuthenticated()` → `throw redirect({ to: '/auth/login' })`;
 *    authenticated → no throw.
 *  - layout navigation visibility: `AdminLayout` renders `admin-nav` with links
 *    to `/admin` and `/admin/low-recall`.
 *  - list load wiring: MSW returns documents → admin renders `document-row`s via
 *    the assembled `DocumentTable` (assembly smoke, not FE-T03's internal coverage).
 *  - `UploadDocument.onUploaded` → admin's `refreshList` (second listDocuments call).
 *  - `BatchActions.onCompleted` → clears `selectedIds` + `refreshList` (select-all
 *    checkbox goes from checked → unchecked, plus a second listDocuments call).
 *  - status filter change → clears `selectedIds` (data-loss guard).
 *  - list-level failure → admin owns `document-list-error` testid (NOT the table's
 *    `error-message`).
 *
 * DEVIATION from FE-D06 spec text: the admin page does NOT render a
 * `document-list-empty` testid — the empty state is delegated to `DocumentTable`'s
 * `empty-state` testid (so the status filter stays reachable when the filtered set
 * is empty). Empty-list assertions target `empty-state`, not `document-list-empty`.
 *
 * The Document Management component uses no TanStack Router hooks (only
 * `useDocumentList` + `useState`), so the page-body cases need no router context;
 * only the `beforeLoad` reference is exercised for the guard (mirroring
 * login.test.tsx). The nav-visibility case renders `AdminLayout`, which uses
 * `<Link>`/`<Outlet/>` and therefore requires a router context — provided by a
 * minimal in-memory router built inline (the repo has no shared router test
 * helper; only `render-chat.tsx`'s `renderWithUser` exists).
 */

const BASE_URL = 'http://localhost:3000'
const LIST_URL = `${BASE_URL}/api/documents`
const UPLOAD_URL = `${BASE_URL}/api/documents/upload`
const BATCH_STATUS_URL = `${BASE_URL}/api/documents/batch-status`
const KEY_STORAGE = 'rwiki_api_key'

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

// --- route access via dynamic import (matches login.test.tsx) --------------
// Split sources after FE-D01: guard lives in `route.tsx`, the Document
// Management body in `index.tsx`. Explicit paths avoid the ambiguous
// `@/routes/admin` directory import (Bundler resolution may pick either file).
type BeforeLoadFn = (ctx: unknown) => unknown
let AdminComponent: ComponentType
let beforeLoad: BeforeLoadFn | undefined

const { Route: AdminLayoutRoute } = await import('@/routes/admin/route')
const { Route: AdminIndexRoute } = await import('@/routes/admin/index')

AdminComponent = AdminIndexRoute.options.component as ComponentType
beforeLoad = AdminLayoutRoute.options.beforeLoad as BeforeLoadFn | undefined

// --- shared MSW counters ---------------------------------------------------
let listCallCount: number

function installListHandler(docs: DocumentListItem[]) {
  server.use(
    http.get(LIST_URL, () => {
      listCallCount += 1
      return HttpResponse.json({ documents: docs })
    }),
  )
}

beforeEach(() => {
  localStorage.clear()
  client.setConfig({ baseUrl: BASE_URL })
  listCallCount = 0
  vi.clearAllMocks()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('admin route — beforeLoad guard', () => {
  it('redirects to /auth/login when not authenticated', () => {
    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()

    let thrown: unknown
    expect(() => {
      try {
        beforeLoad!(undefined)
      } catch (e) {
        thrown = e
        throw e
      }
    }).toThrow()

    expect(isRedirect(thrown!)).toBe(true)
    expect((thrown as { options: { to: string } }).options.to).toBe(
      '/auth/login',
    )
  })

  it('does not redirect when authenticated', () => {
    localStorage.setItem(KEY_STORAGE, 'any-stored-key')

    expect(() => beforeLoad!(undefined)).not.toThrow()
  })
})

describe('admin layout — navigation visibility', () => {
  // AdminLayout renders `<Link>`/`<Outlet/>` from @tanstack/react-router, both of
  // which need router context. The repo has no shared router test helper, so we
  // build a minimal in-memory router inline: AdminLayout sits on a `/admin`
  // parent route with stub child routes for `/admin/` and `/admin/low-recall` so
  // the `<Link to=...>` targets resolve (Link throws if no route matches `to`).
  function renderAdminLayoutInRouter() {
    // AdminLayout itself is the `/admin` route component, with stub child
    // routes for `/admin/` and `/admin/low-recall` so the `<Link to=...>`
    // targets resolve (Link throws if no route matches `to`).
    const rootRoute = createRootRoute({
      // Root must render an <Outlet/> for child routes to mount; rendering null
      // would swallow the matched `/admin` subtree.
      component: () => <Outlet />,
    })
    const adminRoute = createRoute({
      getParentRoute: () => rootRoute,
      path: '/admin',
      component: AdminLayout,
    })
    const adminIndexRoute = createRoute({
      getParentRoute: () => adminRoute,
      path: '/',
      component: () => <div data-testid="outlet-stub-index" />,
    })
    const adminLowRecallRoute = createRoute({
      getParentRoute: () => adminRoute,
      path: '/low-recall',
      component: () => <div data-testid="outlet-stub-low-recall" />,
    })

    const routeTree = rootRoute.addChildren([
      adminRoute.addChildren([adminIndexRoute, adminLowRecallRoute]),
    ])

    const router = createRouter({
      routeTree,
      history: createMemoryHistory({ initialEntries: ['/admin'] }),
    })

    return render(<RouterProvider router={router} />)
  }

  it('renders admin-nav with links to /admin and /admin/low-recall', async () => {
    renderAdminLayoutInRouter()

    // RouterProvider commits the matched route asynchronously, so wait for the
    // layout (and its nav) to mount before asserting.
    const nav = await screen.findByTestId('admin-nav')
    expect(nav).toBeInTheDocument()

    // Both nav entries are reachable by their visible labels.
    const docManagementLink = await screen.findByRole('link', {
      name: /Document Management/i,
    })
    const lowRecallLink = await screen.findByRole('link', {
      name: /Low-Recall Records/i,
    })

    // The hrefs point at the two admin destinations.
    // jsdom resolves TanStack <Link> hrefs to absolute URLs at the current origin.
    expect(docManagementLink.getAttribute('href')).toContain('/admin')
    expect(lowRecallLink.getAttribute('href')).toContain('/admin/low-recall')
  })
})

describe('admin page — list load assembly', () => {
  it('renders admin-page and document rows after the initial list load', async () => {
    installListHandler([makeDoc('a', 'draft'), makeDoc('b', 'published')])

    render(<AdminComponent />)

    // admin-page is always present (skeleton + content).
    expect(screen.getByTestId('admin-page')).toBeInTheDocument()

    // Loading skeleton is shown first, then rows replace it once data resolves.
    const rows = await screen.findAllByTestId('document-row')
    expect(rows).toHaveLength(2)

    // Loading skeleton is gone after the fetch resolves.
    expect(screen.queryByTestId('document-list-loading')).toBeNull()
    expect(listCallCount).toBe(1)
  })

  it('shows the loading skeleton before the first fetch resolves', () => {
    installListHandler([])

    render(<AdminComponent />)

    expect(screen.getByTestId('document-list-loading')).toBeInTheDocument()
  })

  it('shows document-list-error when listDocuments fails', async () => {
    server.use(
      http.get(LIST_URL, () =>
        HttpResponse.json({ code: 500, message: 'boom' }, { status: 500 }),
      ),
    )

    render(<AdminComponent />)

    // Admin owns the list-level error testid (NOT the table's error-message).
    expect(await screen.findByTestId('document-list-error')).toBeVisible()
    expect(screen.queryByTestId('document-row')).toBeNull()
  })

  it('renders the table empty-state (NOT document-list-empty) when the list is empty', async () => {
    // DEVIATION: admin page delegates empty state to DocumentTable's
    // `empty-state` testid; `document-list-empty` does not exist.
    installListHandler([])

    render(<AdminComponent />)

    expect(await screen.findByTestId('empty-state')).toBeVisible()
    expect(screen.queryByTestId('document-list-empty')).toBeNull()
    expect(screen.queryByTestId('document-row')).toBeNull()
  })
})

describe('admin page — upload-success refresh wiring', () => {
  it('refreshes the document list when UploadDocument onUploaded fires', async () => {
    installListHandler([makeDoc('a', 'draft')])

    const user = userEvent.setup()
    render(<AdminComponent />)

    // Wait for the mount fetch to complete before driving the upload.
    await screen.findAllByTestId('document-row')
    expect(listCallCount).toBe(1)

    // Real upload interaction: pick a file, click upload, MSW returns 201.
    server.use(
      http.post(UPLOAD_URL, () =>
        HttpResponse.json({ document: { id: 'b' } }, { status: 201 }),
      ),
    )

    const fileInput = screen.getByTestId('file-input') as HTMLInputElement
    const file = new File(['content'], 'upload.pdf', { type: 'application/pdf' })
    await user.upload(fileInput, file)

    await act(async () => {
      await user.click(screen.getByTestId('upload-button'))
    })

    // The admin's onUploaded (= refreshList) must have triggered a second
    // listDocuments call.
    await waitFor(() => {
      expect(listCallCount).toBe(2)
    })
  })
})

describe('admin page — batch-completed clears selection + refreshes', () => {
  it('clears the selection and re-fetches the list after a successful batch publish', async () => {
    // Two draft docs so the batch-publish button is enabled once selected.
    installListHandler([makeDoc('a', 'draft'), makeDoc('b', 'draft')])

    const user = userEvent.setup()
    render(<AdminComponent />)

    await screen.findAllByTestId('document-row')
    expect(listCallCount).toBe(1)

    // Select all rows via the table's select-all checkbox.
    const selectAll = screen.getByTestId(
      'select-all-checkbox',
    ) as HTMLInputElement
    await user.click(selectAll)

    // Sanity: both ids are now selected.
    expect(
      screen.getAllByTestId('document-select-checkbox').every(
        (cb) => (cb as HTMLInputElement).checked,
      ),
    ).toBe(true)

    // Successful batch publish → triggers BatchActions.onCompleted.
    server.use(
      http.post(BATCH_STATUS_URL, () =>
        HttpResponse.json(
          {
            results: [
              {
                documentId: 'a',
                action: 'publish',
                applied: true,
                status: 'published',
                reason: null,
              },
              {
                documentId: 'b',
                action: 'publish',
                applied: true,
                status: 'published',
                reason: null,
              },
            ],
          },
          { status: 200 },
        ),
      ),
    )

    await act(async () => {
      await user.click(screen.getByTestId('batch-publish-button'))
    })

    // onCompleted clears selectedIds → select-all checkbox becomes unchecked.
    await waitFor(() => {
      expect(
        (screen.getByTestId('select-all-checkbox') as HTMLInputElement).checked,
      ).toBe(false)
    })

    // onCompleted also calls refreshList → a second listDocuments request.
    await waitFor(() => {
      expect(listCallCount).toBe(2)
    })
  })
})

describe('admin page — filter change clears selection (data-loss guard)', () => {
  it('clears selectedIds when the status filter changes', async () => {
    // One draft + one published so switching the filter changes the visible set.
    installListHandler([makeDoc('a', 'draft'), makeDoc('b', 'published')])

    const user = userEvent.setup()
    render(<AdminComponent />)

    await screen.findAllByTestId('document-row')

    // Select the draft row.
    const draftCheckbox = screen.getAllByTestId(
      'document-select-checkbox',
    )[0] as HTMLInputElement
    await user.click(draftCheckbox)
    expect(draftCheckbox.checked).toBe(true)

    // Switch the status filter to 'published' → the draft becomes hidden.
    // REGRESSION GUARD: selection MUST be cleared; otherwise a later delete
    // would operate on the now-hidden selected draft (data loss).
    await user.selectOptions(
      screen.getByTestId('status-filter-select'),
      'published',
    )

    // Only the published row is visible now, and nothing is selected.
    expect(screen.getAllByTestId('document-row')).toHaveLength(1)
    expect(
      (screen.getAllByTestId('document-select-checkbox')[0] as HTMLInputElement)
        .checked,
    ).toBe(false)
    // The batch buttons must be disabled (no selection in scope).
    expect(screen.getByTestId('batch-delete-button')).toBeDisabled()
  })
})
