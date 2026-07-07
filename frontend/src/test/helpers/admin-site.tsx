import { type ReactNode } from 'react'
import { http, HttpResponse } from 'msw'
import { waitFor } from '@testing-library/react'

import { server } from '@/test/mocks/server'
import { AdminSiteProvider } from '@/lib/admin-site-context'
import { useAdminSite } from '@/lib/admin-site-context'

/**
 * FE-T03 — shared admin-site test harness.
 *
 * `AdminSiteProvider` (FE-D03) owns the site context and initializes `siteId`
 * via an async `listSites()` on mount. Several component/hook suites (batch,
 * upload) need a controlled, synchronous-ish `siteId` without every case
 * re-running that bootstrap. The production context (`AdminSiteContext`) is
 * module-private, so we cannot inject a value directly without modifying prod.
 *
 * This helper therefore exposes the real `AdminSiteProvider` and seeds it via
 * MSW `/api/sites`:
 *   - `seedSite('site-a')` → provider auto-selects `site-a` (default-first).
 *   - `seedEmptySites()` → provider keeps `siteId=null` (empty-list anomaly).
 *
 * `waitForSiteReady` resolves once the injected `siteId` is observable through
 * `useAdminSite()` so component tests can drive interactions against a settled
 * selection (mirrors the admin layout's own ready state). For the null case,
 * callers assert the disabled state directly (no ready wait).
 *
 * Approach note (handoff): test-injection goes through the real provider +
 * MSW /api/sites rather than a test-only context stub, because the prod context
 * is private and must not be exported solely for tests.
 */

export const ADMIN_SITE_URL = 'http://localhost:3000/api/sites'

/** MSW /api/sites returning a single site so the provider selects it. */
export function seedSite(siteId: string): void {
  server.use(
    http.get(ADMIN_SITE_URL, () =>
      HttpResponse.json({ sites: [{ id: siteId, name: siteId }] }),
    ),
  )
}

/** MSW /api/sites returning an empty list so the provider keeps siteId=null. */
export function seedEmptySites(): void {
  server.use(
    http.get(ADMIN_SITE_URL, () => HttpResponse.json({ sites: [] })),
  )
}

/** Wrap children in the real AdminSiteProvider. */
export function withAdminSiteProvider({
  children,
}: {
  children: ReactNode
}) {
  return <AdminSiteProvider>{children}</AdminSiteProvider>
}

/**
 * Resolves once the provider has selected a non-null siteId. Used to gate
 * component interactions on a settled selection. Probes via a throwaway
 * `renderHook` is avoided here; instead callers `findByTestId` the provider's
 * own UI, or assert against the rendered component's effects.
 */
export async function waitForSiteReady(
  readSiteId: () => string | null,
): Promise<void> {
  await waitFor(() => {
    expect(readSiteId()).not.toBeNull()
  })
}

// Re-exported for suites that need to read the context directly.
export { useAdminSite }
