import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { renderHook, waitFor } from '@testing-library/react'

import type { useWidgetSuggestions } from '../use-widget-suggestions'

/**
 * FE-T01 — useWidgetSuggestions multi-channel contract tests.
 *
 * Guards the FE-D01 invariants for the widget suggestions hook:
 *  (a) the request URL carries `channelId` (correctly URL-encoded);
 *  (b) the module-level cache is keyed by `${channelId}:${locale}`, so different
 *      channels never share suggestion data and a channel-returned empty array is
 *      cached (no refetch on re-mount);
 *  (c) CHANNEL-STRICT: an empty server array (or a fetch failure) MUST NOT fall
 *      back to the local `_fallback` suggestedQuestions — the fourth param is
 *      intentionally ignored.
 *
 * The hook holds a module-level `cache: Map<string,string[]>` that is NOT
 * exported, so each test isolates module state via `vi.resetModules()` + a
 * dynamic `import('../use-widget-suggestions')` (mirrors main.test.tsx's
 * `await import` pattern), guaranteeing an empty cache at test start.
 */

const API_URL = 'http://localhost:3000'

/** Builds a fetch Response returning the given JSON body. */
function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
  })
}

async function renderSuggestionsHook(
  channelId: string | string[],
  locale: 'en' | 'zh-CN' = 'en',
  fallback?: string[],
  apiUrl = API_URL,
) {
  const mod = await import('../use-widget-suggestions')
  type UseSuggestions = typeof useWidgetSuggestions
  const useFn = mod.useWidgetSuggestions as unknown as UseSuggestions
  const channelIds = Array.isArray(channelId) ? channelId : [channelId]
  return renderHook(() => useFn(apiUrl, locale, channelIds, fallback))
}

describe('useWidgetSuggestions', () => {
  let fetchSpy: ReturnType<typeof vi.fn>

  beforeEach(() => {
    // Isolate the module-level cache so every test starts with an empty cache.
    vi.resetModules()
    fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)
  })

  afterEach(() => {
    vi.restoreAllMocks()
    vi.unstubAllGlobals()
  })

  // ── (a) URL contract: channelId is present + URL-encoded ──────────────────
  describe('request URL carries channelId', () => {
    it('appends channelId (and locale) as query params on the suggestions request', async () => {
      fetchSpy.mockResolvedValue(jsonResponse({ questions: [] }))

      await renderSuggestionsHook('channel-a', 'en')

      await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1))

      const url = fetchSpy.mock.calls[0]![0] as string
      const parsed = new URL(url)
      expect(parsed.pathname).toBe('/api/chat/suggestions')
      expect(parsed.searchParams.get('channelId')).toBe('channel-a')
      expect(parsed.searchParams.get('locale')).toBe('en')
    })

    it('URL-encodes a channelId containing special characters (e.g. "a&b")', async () => {
      fetchSpy.mockResolvedValue(jsonResponse({ questions: [] }))

      await renderSuggestionsHook('a&b', 'en')

      await waitFor(() => expect(fetchSpy).toHaveBeenCalledTimes(1))

      const url = fetchSpy.mock.calls[0]![0] as string
      // The raw ampersand must be percent-encoded so it cannot be parsed as a
      // new param boundary.
      const parsed = new URL(url)
      expect(parsed.searchParams.get('channelId')).toBe('a&b')
      expect(url).toContain(encodeURIComponent('a&b'))
    })
  })

  // ── (b) cache key isolation + empty-array caching ────────────────────────
  describe('cache-key isolation by channelId', () => {
    it('caches per channelId: channel-b misses the cache and refetches, switching back to channel-a hits the cache', async () => {
      // First mount: channel-a → fetch returns ['q-a']
      fetchSpy.mockResolvedValueOnce(jsonResponse({ questions: ['q-a'] }))
      const a1 = await renderSuggestionsHook('channel-a')
      await waitFor(() => expect(a1.result.current).toEqual(['q-a']))

      // Second mount: channel-b → cache key differs, so it MUST refetch
      fetchSpy.mockResolvedValueOnce(jsonResponse({ questions: ['q-b'] }))
      const b1 = await renderSuggestionsHook('channel-b')
      await waitFor(() => expect(b1.result.current).toEqual(['q-b']))
      const callsAfterB = fetchSpy.mock.calls.length

      // Third mount: channel-a again → cache HIT, no additional fetch, returns cached ['q-a']
      const a2 = await renderSuggestionsHook('channel-a')
      await waitFor(() => expect(a2.result.current).toEqual(['q-a']))
      expect(fetchSpy.mock.calls.length).toBe(callsAfterB) // no new fetch
    })

    it('caches an empty array so a re-mount with the same channelId does not refetch', async () => {
      fetchSpy.mockResolvedValueOnce(jsonResponse({ questions: [] }))
      const first = await renderSuggestionsHook('channel-a')
      await waitFor(() => expect(first.result.current).toEqual([]))
      const callsAfterFirst = fetchSpy.mock.calls.length
      expect(callsAfterFirst).toBe(1)

      // Second mount, same channelId → empty array is cached, so no refetch
      const second = await renderSuggestionsHook('channel-a')
      await waitFor(() => expect(second.result.current).toEqual([]))
      expect(fetchSpy.mock.calls.length).toBe(callsAfterFirst) // still 1
    })
  })

  // ── (c) CHANNEL-STRICT: never fall back to local _fallback ───────────────
  describe('channel-strict: server result wins, _fallback is ignored', () => {
    it('returns [] when the server returns {questions: []}, even when a _fallback is supplied', async () => {
      fetchSpy.mockResolvedValue(jsonResponse({ questions: [] }))

      const { result } = await renderSuggestionsHook('channel-a', 'en', [
        'local-fallback',
      ])

      await waitFor(() => expect(result.current).toEqual([]))
      // The load-bearing assertion: no 'local-fallback' bleeds through.
      expect(result.current).not.toContain('local-fallback')
    })

    it('returns [] on fetch failure even when a _fallback is supplied', async () => {
      fetchSpy.mockRejectedValue(new TypeError('Failed to fetch'))

      const { result } = await renderSuggestionsHook('channel-a', 'en', [
        'local-fallback',
      ])

      await waitFor(() => expect(result.current).toEqual([]))
      expect(result.current).not.toContain('local-fallback')
    })
  })
})
