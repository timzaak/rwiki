import { describe, it, expect } from 'vitest'
import { renderHook } from '@testing-library/react'

import { ChannelIdProvider, useChannelId } from '@/components/chat/channel-id-context'

/**
 * FE-T02 — `useChannelId` / `ChannelIdProvider` contract (FE-D02 point a/d).
 *
 * Pure context logic; no MSW, no router, no network. The hook is the single
 * source of the main-site channelId that `use-chat-stream` / `use-feedback`
 * consume, so its contract is load-bearing:
 *  - returns the provider's channelId inside a provider;
 *  - throws when there is no provider (forces `/c/$channelId` to validate + wrap
 *    before rendering chat chrome — unknown state must not reach these hooks);
 *  - different channelId props pass through unchanged.
 */
describe('useChannelId', () => {
  it('returns the channelId provided by ChannelIdProvider', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <ChannelIdProvider channelId="channel-a">{children}</ChannelIdProvider>
    )

    const { result } = renderHook(() => useChannelId(), { wrapper })

    expect(result.current).toBe('channel-a')
  })

  it('throws when called outside of a ChannelIdProvider', () => {
    // No wrapper → no provider in the tree → useChannelId must throw. We assert
    // throw behavior, not the exact message (the wording is an impl detail;
    // the contract is "refuse to run without a provider").
    expect(() => renderHook(() => useChannelId())).toThrow()
  })

  it('passes through a different channelId prop', () => {
    const wrapper = ({ children }: { children: React.ReactNode }) => (
      <ChannelIdProvider channelId="channel-b">{children}</ChannelIdProvider>
    )

    const { result } = renderHook(() => useChannelId(), { wrapper })

    expect(result.current).toBe('channel-b')
  })
})
