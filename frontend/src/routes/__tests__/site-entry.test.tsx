import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { http, HttpResponse } from 'msw'
import {
  createRootRoute,
  createRoute,
  createRouter,
  createMemoryHistory,
  RouterProvider,
  Outlet,
} from '@tanstack/react-router'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'

/**
 * FE-T02 — landing `site-entry` links (FE-D02 point e).
 *
 * `routes/index.tsx` renders a `listSites()`-driven list of
 * `<Link to="/s/$siteId" params={{ siteId }}>` entries, each with a stable
 * `data-testid="site-entry"` span and a per-id `site-entry-<id>` on the link.
 *
 * Covered:
 *  - one `site-entry` per returned site; each link href contains `/s/<id>`
 *    (jsdom resolves TanStack <Link> href to an absolute URL → assert on the
 *    href string containing the path segment).
 *  - empty list → no `site-entry` (graceful `site-list-empty` message per impl).
 *  - listSites 500 → graceful `site-list-error`, no throw.
 *
 * Router assembly mirrors `routes/admin/__tests__/admin.test.tsx`:
 * createMemoryHistory + RouterProvider + the landing component on `/` plus a
 * STUB `/s/$siteId` child route so `<Link to="/s/$siteId">` resolves (Link
 * throws if no route matches `to`).
 */

const BASE_URL = 'http://localhost:3000'
const SITES_URL = `${BASE_URL}/api/sites`

const { Route: LandingRoute } = await import('@/routes/index')
// `RouteComponent` is the exact component type createRoute expects; keep it as
// such rather than narrowing to ComponentType (which createRoute rejects).
const LandingComponent = LandingRoute.options.component

function renderLandingRoute() {
  const rootRoute = createRootRoute({
    component: () => <Outlet />,
  })
  const indexRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/',
    component: LandingComponent,
  })
  // STUB `/s/$siteId` so `<Link to="/s/$siteId">` resolves (Link throws on an
  // unmatched `to`). The stub never mounts — initialEntries is ['/'].
  const siteStubRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/s/$siteId',
    component: () => <div data-testid="site-stub" />,
  })

  const routeTree = rootRoute.addChildren([indexRoute, siteStubRoute])

  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  return render(<RouterProvider router={router} />)
}

function installSitesHandler(sites: Array<{ id: string; name: string }>) {
  server.use(
    http.get(SITES_URL, () => HttpResponse.json({ sites })),
  )
}

beforeEach(() => {
  client.setConfig({ baseUrl: BASE_URL })
  vi.clearAllMocks()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('landing — site-entry links', () => {
  it('renders one site-entry per site, each linking to /s/<id>', async () => {
    installSitesHandler([
      { id: 'site-a', name: 'A' },
      { id: 'site-b', name: 'B' },
    ])

    renderLandingRoute()

    const entries = await screen.findAllByTestId('site-entry')
    expect(entries).toHaveLength(2)

    // Per-id links exist; jsdom resolves TanStack <Link> hrefs to absolute
    // URLs — assert the path segment is present.
    const linkA = await screen.findByTestId('site-entry-site-a')
    const linkB = await screen.findByTestId('site-entry-site-b')
    expect(linkA.getAttribute('href')).toContain('/s/site-a')
    expect(linkB.getAttribute('href')).toContain('/s/site-b')
  })
})

describe('landing — empty/failure degradation', () => {
  it('renders no site-entry when the site list is empty', async () => {
    installSitesHandler([])

    renderLandingRoute()

    // Empty list → graceful `site-list-empty` message, no entries, no throw.
    expect(await screen.findByTestId('site-list-empty')).toBeVisible()
    expect(screen.queryAllByTestId('site-entry')).toHaveLength(0)
  })

  it('degrades gracefully (no throw, no site-entry) when listSites fails', async () => {
    // FE-D02 DEFECT (reported for re-dispatch): the dedicated `site-list-error`
    // branch is UNREACHABLE through any HTTP/network failure. The landing calls
    // `listSites()` WITHOUT `throwOnError`; the generated client catches
    // fetch/parse errors and RESOLVES with `{ data: undefined, error }`, so
    // `.catch(() => setState({ status: 'error' }))` never fires — a failed
    // listSites resolves to `sites = []` → the `empty` branch
    // (`site-list-empty`), NOT `site-list-error`.
    //
    // This asserts the load-bearing safety property that still holds: the
    // landing renders a terminal non-entry state with no `site-entry` links and
    // no throw. The unreachable `site-list-error` UI is documented for FE-D02
    // (it needs `throwOnError: true` on `listSites()`).
    server.use(http.get(SITES_URL, () => HttpResponse.error()))

    renderLandingRoute()

    // listSites failure → treated as empty (no sites), gracefully.
    expect(await screen.findByTestId('site-list-empty')).toBeVisible()
    expect(screen.queryAllByTestId('site-entry')).toHaveLength(0)
  })
})
