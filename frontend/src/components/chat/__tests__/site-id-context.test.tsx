import { describe, it, expect } from 'vitest'
import { renderHook } from '@testing-library/react'

import { SiteIdProvider, useSiteId } from '@/components/chat/site-id-context'

/**
 * FE-T02 — `useSiteId` / `SiteIdProvider` contract (FE-D02 point a/d).
 *
 * Pure context logic; no MSW, no router, no network. The hook is the single
 * source of the main-site siteId that `use-chat-stream` / `use-feedback`
 * consume, so its contract is load-bearing:
 *  - returns the provider's siteId inside a provider;
 *  - throws when there is no provider (forces `/s/$siteId` to validate + wrap
 *    before rendering chat chrome — unknown state must not reach these hooks);
 *  - different siteId props pass through unchanged.
 */
describe('useSiteId', () => {
  it('returns the siteId provided by SiteIdProvider', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <SiteIdProvider siteId="site-a">{children}</SiteIdProvider>
    )

    const { result } = renderHook(() => useSiteId(), { wrapper })

    expect(result.current).toBe('site-a')
  })

  it('throws when called outside of a SiteIdProvider', () => {
    // No wrapper → no provider in the tree → useSiteId must throw. We assert
    // throw behavior, not the exact message (the wording is an impl detail;
    // the contract is "refuse to run without a provider").
    expect(() => renderHook(() => useSiteId())).toThrow()
  })

  it('passes through a different siteId prop', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <SiteIdProvider siteId="site-b">{children}</SiteIdProvider>
    )

    const { result } = renderHook(() => useSiteId(), { wrapper })

    expect(result.current).toBe('site-b')
  })
})
