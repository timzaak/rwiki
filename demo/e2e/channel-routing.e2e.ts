/**
 * Main-route channel routing contract E2E (support-multiple-website) — DE-D04
 *
 * Covers the routing/validation closure of `/` and `/c/$channelId`:
 *  - US-INTG-005: channel-entry discovery on root `/`.
 *  - US-INTG-007 scn 2 + design §4.4.2: unknown channel renders `channel-not-found`
 *    ("频道不存在或不可用") and fires ZERO chat / suggestions / feedback requests.
 *
 * US-story traceability (DRAFT source, pre-publish — do NOT treat as published
 * fact): `.ai/user-stories/integration/support-multiple-website.md`
 *   - US-INTG-005 "通过配置文件定义可接入的频道" (scn 1 "成功配置多个频道")
 *   - US-INTG-007 scn 2 "访问未配置频道"
 * Design reference: `.ai/design/support-multiple-website.md` §4.4.2 (主站频道路由:
 *   unknown channel shows "频道不存在或不可用" and does NOT initiate chat / suggestions /
 *   feedback requests).
 *
 * Boundary:
 *  - The positive known-channel chat load (US-INTG-007 scn 1 "/c/channel-a 访问频道") is
 *    already exercised by DE-D01's `chat-rag-streaming.e2e.ts`; do NOT duplicate
 *    it here. This file covers the residual routing negatives + discovery only.
 *  - The widget init contract is DE-D03; the document API isolation is DE-D02.
 *
 * Dependencies:
 *  - demo/e2e/fixtures/chat.fixtures.ts (demoLogger via fixture; FRONTEND_URL)
 *  - demo/e2e/pages/home-page.ts (channel-entry-list POM, reused from DE-D01)
 *  - demo/e2e/selectors.ts (SELECTORS.channel.*, SELECTORS.chat.floatingButton)
 *
 * Selector calibration (verified against frontend source BEFORE authoring):
 *  - `frontend/src/routes/index.tsx`: `channel-list-loading` (fetch pending),
 *    `channel-entry-${id}` (the per-channel link), `channel-entry` (the name span).
 *  - `frontend/src/routes/c/$channelId.tsx`: `channel-loading` (listChannels pending),
 *    `channel-not-found` (unknown channel — REAL text "频道不存在或不可用"), and the
 *    `ready` branch that mounts `<FloatingButton visible />` (the chat-ready
 *    signal; absent for unknown/error channels).
 *
 * Known prior-phase residual (FE-A01 / FE-D02): `listChannels()` lacks
 * `throwOnError`, so a failing `GET /api/channels` resolves (never rejects) to
 * `{ data: undefined, error }` → `channels = []` → the `/c/$channelId` route renders
 * the `unknown`/`channel-not-found` branch, NOT `channel-error`. Consequently the
 * `channel-error` + retry UI is currently UNREACHABLE. The load-bearing unknown-channel
 * ZERO-request hard-constraint is UNAFFECTED (unknown channel still renders
 * `channel-not-found`). See handoff for why the optional channel-error retry test is
 * OMITTED.
 */

import { test, expect } from './fixtures/chat.fixtures'
import { HomePage } from './pages/home-page'
import { SELECTORS } from './selectors'
import { FRONTEND_URL } from './fixtures/chat.fixtures'

/**
 * Matches the chat API family that an unknown channel must NEVER request:
 *  - POST `/api/chat`            (streaming chat)
 *  - GET  `/api/chat/suggestions` (per-channel suggested questions)
 *  - POST `/api/chat/feedback`    (thumbs up/down)
 *
 * The pattern `/api/chat` followed by end-of-path, a `/`, or a `?` covers all
 * three endpoints without matching unrelated paths (e.g. `/api/channels`).
 * This single check implements design §4.4.2's hard-constraint.
 */
const CHAT_API_PATTERN = /\/api\/chat(?:\/|$|\?)/

// ---------------------------------------------------------------------------
// US-INTG-005 — channel-entry discovery on root `/`
// ---------------------------------------------------------------------------
test.describe('US-INTG-005: channel-entry discovery on root /', () => {
  test('root / lists configured channels and clicking help_center navigates to /c/help_center with chat ready', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const homePage = new HomePage(page)
    await homePage.navigate()

    // The list briefly shows a loading placeholder while fetching /api/channels;
    // the ready state renders the channel entries.
    await expect(page.locator(SELECTORS.channel.channelListLoading)).toBeHidden({
      timeout: 15000,
    })

    // Assert: BOTH demo-seeded channel entries render.
    // (US-INTG-005 scn 1 "成功配置多个频道": `backend/config/demo.toml` defines
    // `[channels.help_center]` + `[channels.developer_docs]`, so both entries must
    // appear on the discovery list.)
    await expect(page.locator(SELECTORS.channel.channelEntryById('help_center'))).toBeVisible()
    await expect(page.locator(SELECTORS.channel.channelEntryById('developer_docs'))).toBeVisible()

    // Click the help_center entry (reuses the DE-D01 HomePage POM) and assert
    // navigation lands on `/c/help_center` with the chat ready state.
    await homePage.clickChannelEntry('help_center')
    await expect(page).toHaveURL(/\/c\/help_center\/?$/)

    // The `/c/$channelId` route only renders `<FloatingButton visible />` once
    // `listChannels()` confirms the channel (the `ready` branch). Its visibility is
    // the stable "channel resolved + chat ready" signal.
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeVisible({
      timeout: 15000,
    })
  })
})

// ---------------------------------------------------------------------------
// US-INTG-007 scn 2 + design §4.4.2 — unknown channel hard-constraint
// PRIMARY ACCEPTANCE: unknown channel fires ZERO chat / suggestions / feedback.
// grep pattern for DE-D05: `unknown channel`
// ---------------------------------------------------------------------------
test.describe('US-INTG-007 scn 2: unknown channel hard-constraint (ZERO chat/suggestions/feedback)', () => {
  test('unknown channel renders channel-not-found and fires ZERO chat/suggestions/feedback requests', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    // PRIMARY ACCEPTANCE (design §4.4.2): an unknown channel must NOT initiate any
    // chat / suggestions / feedback request. The `/c/$channelId` route only mounts
    // the chat infrastructure (ChannelIdProvider + FloatingButton +
    // ChannelChatModalMount, which owns the suggestions call) once `listChannels()`
    // confirms the channel. An unknown channel resolves to the `unknown` branch
    // (`channel-not-found`) and never mounts it — so none of these endpoints can
    // be requested. We observe ALL requests and fail-on-match.
    const forbidden: { method: string; url: string }[] = []
    page.on('request', (req) => {
      if (CHAT_API_PATTERN.test(req.url())) {
        forbidden.push({ method: req.method(), url: req.url() })
      }
    })

    await page.goto(`${FRONTEND_URL}/c/channel-unknown`)

    // Assert: the unknown-channel surface renders with the contract text.
    const notFound = page.locator(SELECTORS.channel.channelNotFound)
    await expect(notFound).toBeVisible({ timeout: 15000 })
    await expect(notFound).toContainText('频道不存在或不可用')

    // Assert: the loading state has settled — `channel-loading` is gone (the
    // `unknown` branch replaces it; `channel-loading` is no longer in the DOM).
    await expect(page.locator(SELECTORS.channel.channelLoading)).toBeHidden()

    // Allow a settle window to capture any lazily-triggered requests before the
    // hard assertion. The route never reaches `ready` for an unknown channel, so
    // none should fire — this guard makes a regression deterministic.
    // (networkidle is best-effort; non-fatal if it cannot settle.)
    await page.waitForLoadState('networkidle').catch(() => undefined)

    // PRIMARY ASSERTION — do NOT weaken: ZERO chat / suggestions / feedback
    // requests for the unknown channel.
    expect(
      forbidden,
      `Expected ZERO chat/suggestions/feedback requests for the unknown channel, but observed: ${JSON.stringify(forbidden)}`,
    ).toEqual([])
  })
})
