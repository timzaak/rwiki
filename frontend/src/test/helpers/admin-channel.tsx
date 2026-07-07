import { type ReactNode } from 'react'
import { http, HttpResponse } from 'msw'
import { waitFor } from '@testing-library/react'

import { server } from '@/test/mocks/server'
import { AdminChannelProvider } from '@/lib/admin-channel-context'
import { useAdminChannel } from '@/lib/admin-channel-context'

/**
 * FE-T03 — shared admin-channel test harness.
 *
 * `AdminChannelProvider` (FE-D03) owns the channel context and initializes `channelId`
 * via an async `listChannels()` on mount. Several component/hook suites (batch,
 * upload) need a controlled, synchronous-ish `channelId` without every case
 * re-running that bootstrap. The production context (`AdminChannelContext`) is
 * module-private, so we cannot inject a value directly without modifying prod.
 *
 * This helper therefore exposes the real `AdminChannelProvider` and seeds it via
 * MSW `/api/channels`:
 *   - `seedChannel('channel-a')` → provider auto-selects `channel-a` (default-first).
 *   - `seedEmptyChannels()` → provider keeps `channelId=null` (empty-list anomaly).
 *
 * `waitForChannelReady` resolves once the injected `channelId` is observable through
 * `useAdminChannel()` so component tests can drive interactions against a settled
 * selection (mirrors the admin layout's own ready state). For the null case,
 * callers assert the disabled state directly (no ready wait).
 *
 * Approach note (handoff): test-injection goes through the real provider +
 * MSW /api/channels rather than a test-only context stub, because the prod context
 * is private and must not be exported solely for tests.
 */

export const ADMIN_CHANNEL_URL = 'http://localhost:3000/api/channels'

/** MSW /api/channels returning a single channel so the provider selects it. */
export function seedChannel(channelId: string): void {
  server.use(
    http.get(ADMIN_CHANNEL_URL, () =>
      HttpResponse.json({ channels: [{ id: channelId, name: channelId }] }),
    ),
  )
}

/** MSW /api/channels returning an empty list so the provider keeps channelId=null. */
export function seedEmptyChannels(): void {
  server.use(
    http.get(ADMIN_CHANNEL_URL, () => HttpResponse.json({ channels: [] })),
  )
}

/** Wrap children in the real AdminChannelProvider. */
export function withAdminChannelProvider({
  children,
}: {
  children: ReactNode
}) {
  return <AdminChannelProvider>{children}</AdminChannelProvider>
}

/**
 * Resolves once the provider has selected a non-null channelId. Used to gate
 * component interactions on a settled selection. Probes via a throwaway
 * `renderHook` is avoided here; instead callers `findByTestId` the provider's
 * own UI, or assert against the rendered component's effects.
 */
export async function waitForChannelReady(
  readChannelId: () => string | null,
): Promise<void> {
  await waitFor(() => {
    expect(readChannelId()).not.toBeNull()
  })
}

// Re-exported for suites that need to read the context directly.
export { useAdminChannel }
