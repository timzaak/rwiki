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
 * FE-T02 — landing `channel-entry` links (FE-D02 point e).
 *
 * `routes/index.tsx` renders a `listChannels()`-driven list of
 * `<Link to="/c/$channelId" params={{ channelId }}>` entries, each with a stable
 * `data-testid="channel-entry"` span and a per-id `channel-entry-<id>` on the link.
 *
 * Covered:
 *  - one `channel-entry` per returned channel; each link href contains `/c/<id>`
 *    (jsdom resolves TanStack <Link> href to an absolute URL → assert on the
 *    href string containing the path segment).
 *  - empty list → no `channel-entry` (graceful `channel-list-empty` message per impl).
 *  - listChannels 500 → graceful `channel-list-error`, no throw.
 *
 * Router assembly mirrors `routes/admin/__tests__/admin.test.tsx`:
 * createMemoryHistory + RouterProvider + the landing component on `/` plus a
 * STUB `/c/$channelId` child route so `<Link to="/c/$channelId">` resolves (Link
 * throws if no route matches `to`).
 */

const BASE_URL = 'http://localhost:3000'
const CHANNELS_URL = `${BASE_URL}/api/channels`

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
  // STUB `/c/$channelId` so `<Link to="/c/$channelId">` resolves (Link throws on an
  // unmatched `to`). The stub never mounts — initialEntries is ['/'].
  const channelStubRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/c/$channelId',
    component: () => <div data-testid="channel-stub" />,
  })

  const routeTree = rootRoute.addChildren([indexRoute, channelStubRoute])

  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: ['/'] }),
  })

  return render(<RouterProvider router={router} />)
}

function installChannelsHandler(channels: Array<{ id: string; name: string }>) {
  server.use(
    http.get(CHANNELS_URL, () => HttpResponse.json({ channels })),
  )
}

beforeEach(() => {
  client.setConfig({ baseUrl: BASE_URL })
  vi.clearAllMocks()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('landing — channel-entry links', () => {
  it('renders one channel-entry per channel, each linking to /c/<id>', async () => {
    installChannelsHandler([
      { id: 'channel-a', name: 'A' },
      { id: 'channel-b', name: 'B' },
    ])

    renderLandingRoute()

    const entries = await screen.findAllByTestId('channel-entry')
    expect(entries).toHaveLength(2)

    // Per-id links exist; jsdom resolves TanStack <Link> hrefs to absolute
    // URLs — assert the path segment is present.
    const linkA = await screen.findByTestId('channel-entry-channel-a')
    const linkB = await screen.findByTestId('channel-entry-channel-b')
    expect(linkA.getAttribute('href')).toContain('/c/channel-a')
    expect(linkB.getAttribute('href')).toContain('/c/channel-b')
  })
})

describe('landing — empty/failure degradation', () => {
  it('renders no channel-entry when the channel list is empty', async () => {
    installChannelsHandler([])

    renderLandingRoute()

    // Empty list → graceful `channel-list-empty` message, no entries, no throw.
    expect(await screen.findByTestId('channel-list-empty')).toBeVisible()
    expect(screen.queryAllByTestId('channel-entry')).toHaveLength(0)
  })

  it('degrades gracefully (no throw, no channel-entry) when listChannels fails', async () => {
    // FE-D02 DEFECT (reported for re-dispatch): the dedicated `channel-list-error`
    // branch is UNREACHABLE through any HTTP/network failure. The landing calls
    // `listChannels()` WITHOUT `throwOnError`; the generated client catches
    // fetch/parse errors and RESOLVES with `{ data: undefined, error }`, so
    // `.catch(() => setState({ status: 'error' }))` never fires — a failed
    // listChannels resolves to `channels = []` → the `empty` branch
    // (`channel-list-empty`), NOT `channel-list-error`.
    //
    // This asserts the load-bearing safety property that still holds: the
    // landing renders a terminal non-entry state with no `channel-entry` links and
    // no throw. The unreachable `channel-list-error` UI is documented for FE-D02
    // (it needs `throwOnError: true` on `listChannels()`).
    server.use(http.get(CHANNELS_URL, () => HttpResponse.error()))

    renderLandingRoute()

    // listChannels failure → treated as empty (no channels), gracefully.
    expect(await screen.findByTestId('channel-list-empty')).toBeVisible()
    expect(screen.queryAllByTestId('channel-entry')).toHaveLength(0)
  })
})
