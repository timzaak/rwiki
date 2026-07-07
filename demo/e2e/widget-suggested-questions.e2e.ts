/**
 * Widget Suggested Questions E2E Tests (support-multiple-website migration)
 *
 * US-story traceability:
 * - US-INTG-004 (configure suggested questions in the Widget) + the
 *   support-multiple-website channel-strongly-controlled-suggestions change
 *   (design §5.6).
 *
 * Source: `.ai/user-stories/integration/support-multiple-website.md` (DRAFT,
 * pre-publish — do NOT treat as published fact). The channel-strongly-controlled
 * rule is also reflected in design §5.6 and §1.4.
 *
 * === What changed (DE-D03) ===
 * `channelId` is now REQUIRED on `RWikiChat.init({ apiUrl, channelId })`. The
 * legacy file called `RWikiChat.init({ apiUrl })` without `channelId`, so the
 * widget no longer renders (`config.ts` logs `[RWikiChat] channelId is required`
 * and returns null). Every `setupWidgetPage` config here now passes
 * `channelId: 'help_center'`.
 *
 * === Removed / converted legacy scenarios (design §5.6) ===
 * The old scenarios asserted CLIENT-SIDE suggestion resolution:
 *   - old scn 1: locale map `{default, en}` → matched "en" buttons.
 *   - old scn 2: simple array `['Question A','Question B']` → those buttons.
 * These are OBSOLETE. Design §5.6 makes suggestions CHANNEL-STRONGLY-CONTROLLED:
 * the widget ignores the locally-passed `suggestedQuestions` (the param is
 * kept for call-site compatibility but `_fallback` is intentionally unused —
 * see `use-widget-suggestions.ts`) and instead fetches
 * `GET /api/chat/suggestions?locale=&channelId=` from the server. The server
 * returns per-channel questions; `developer_docs` is intentionally configured
 * with NO `suggested_questions` and returns `[]`.
 *
 * Therefore the old client-side locale-map / simple-array scenarios were
 * REMOVED (they cannot be salvaged — the contract no longer reads the
 * client `suggestedQuestions` at all). They are replaced by SERVER-side
 * equivalents that assert the help_center server suggestions render.
 *
 * Scenarios (after migration):
 * 1. Widget with channelId='help_center' shows the SERVER-returned suggestions.
 * 2. help_center suggestions render before any message is sent (API-driven).
 * 3. Widget with channelId='developer_docs' (empty server suggestions) shows no
 *    suggestion buttons while chat input still works.
 *
 * IMPORTANT: The widget mounts inside a Shadow DOM. Playwright locator()
 * does NOT pierce Shadow DOM, so all queries use page.evaluate() to
 * access elements inside the shadow root (shared via helpers/widget-helper).
 *
 * Do NOT import or use ChatPage in this file.
 */

import { test, expect } from './fixtures/chat.fixtures'
import {
  setupWidgetPage,
  createWidgetHelper,
  WIDGET_BASE_URL,
  DEFAULT_WIDGET_CHANNEL_ID,
  EMPTY_WIDGET_CHANNEL_ID,
} from './helpers/widget-helper'

test.describe('US-INTG-004: Widget suggested questions (channel-controlled, §5.6)', () => {
  // ---------------------------------------------------------------------------
  // Scenario 1: Widget with channelId='help_center' shows the SERVER-returned
  // suggested questions inside the Shadow DOM. (Replaces old client-side
  // locale-map scenario — design §5.6 channel-strongly-controlled suggestions.)
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 1 - help_center server suggestions render in the widget', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: DEFAULT_WIDGET_CHANNEL_ID,
      // Client-side suggestions are now IGNORED by the widget (§5.6). We pass
      // a bogus value to prove the server's help_center suggestions win.
      suggestedQuestions: ['THIS CLIENT VALUE MUST BE IGNORED'],
    })

    const helper = createWidgetHelper(page)

    // Open the chat modal by clicking the floating button inside the Shadow DOM
    await helper.clickFloatingButton()
    await helper.waitForModal()

    // Wait for the server-returned suggested questions to render.
    await helper.waitForQuestionButtons()

    // Assert: the rendered buttons are the help_center SERVER suggestions
    // (from backend/config/demo.toml `[channels.help_center.suggested_questions]`),
    // NOT the locally-passed (now-ignored) client value.
    const texts = await helper.getQuestionTexts()
    expect(texts.length).toBeGreaterThan(0)
    expect(texts).not.toContain('THIS CLIENT VALUE MUST BE IGNORED')

    // help_center is configured with default/en groups; Playwright launches
    // with `--lang=en-US`, which prefix-matches the "en" group.
    // (verified server-side in suggested-questions.e2e.ts scn 2/3)
    expect(texts).toContain('How to get started')
    expect(texts).toContain('What file formats are supported')
  })

  // ---------------------------------------------------------------------------
  // Scenario 2: help_center suggestions render before any message is sent.
  // Confirms the suggestions come from the API (GET /api/chat/suggestions),
  // available immediately in the empty-state.
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 2 - help_center suggestions appear in empty state before first message', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: DEFAULT_WIDGET_CHANNEL_ID,
    })

    const helper = createWidgetHelper(page)
    await helper.clickFloatingButton()
    await helper.waitForModal()

    // Suggestions are present while the conversation is still empty.
    await helper.waitForQuestionButtons()
    expect(await helper.isModalOpen()).toBe(true)
    expect((await helper.getQuestionTexts()).length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 3: Widget with channelId='developer_docs' (no configured
  // suggested_questions → server returns []) shows no suggestion buttons,
  // while the chat input still works. (Replaces the old "no
  // suggestedQuestions" client-side scenario — now it is the SERVER empty
  // array that drives the empty state, per §5.6.)
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 3 - developer_docs empty server suggestions show no buttons', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: EMPTY_WIDGET_CHANNEL_ID,
    })

    const helper = createWidgetHelper(page)
    await helper.clickFloatingButton()
    await helper.waitForModal()

    // Assert: modal is open
    expect(await helper.isModalOpen()).toBe(true)

    // Assert: no suggested question buttons render (developer_docs server
    // suggestions are an empty array → SuggestedQuestions returns null).
    expect(await helper.hasQuestionButtons()).toBe(false)

    // Assert: chat input is present and functional (widget works normally)
    expect(await helper.hasChatInput()).toBe(true)
  })
})
