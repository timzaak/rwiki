/**
 * Widget Multi-Channel E2E Tests — US-INTG-006
 *
 * Covers the Widget bundle init contract after `channelId` became required, and
 * the US-INTG-006 scenarios from the multi-channel user story.
 *
 * US-story traceability (DRAFT source, pre-publish — do NOT treat as
 * published fact):
 *   `.ai/user-stories/integration/support-multiple-website.md` — US-INTG-006
 * Design references: §4.4.2 (Widget 初始化 / 错误态), §5.6 (channelId required).
 *
 * Boundary: tests target the standalone IIFE widget bundle
 * (`${BASE_URL}/widget/rwiki-chat.js`) loaded via `page.setContent` into a
 * Shadow DOM. Do NOT use ChatPage. Do NOT cover main-route `/c/$channelId`
 * (that is DE-D01/DE-D04). Do NOT modify widget source.
 *
 * IMPORTANT: The widget mounts inside a Shadow DOM. Playwright locator()
 * does NOT pierce Shadow DOM, so all queries use page.evaluate() to access
 * elements inside the shadow root (shared via helpers/widget-helper).
 *
 * === Actual widget behavior this file asserts (verified against source) ===
 *  - scn 2 (no channelId): `validateWidgetConfig` (`frontend/src/widget/config.ts`)
 *    logs `[RWikiChat] channelId is required` and returns null; `init()`
 *    (`frontend/src/widget/main.tsx`) returns early BEFORE creating the
 *    `#rwiki-chat-widget` host div. So the host element does NOT exist and
 *    has no shadowRoot chat content. We assert absence of the host + capture
 *    the console error. No toast is asserted.
 *  - scn 3 (unconfigured channelId 'channel-unknown'): channelId is a non-empty
 *    string, so init() SUCCEEDS and the widget renders. The channel-unavailable
 *    state is only surfaced AFTER the first chat request: the server returns
 *    HTTP 400 `频道 channel-unknown 未配置` (`backend/.../handlers/chat.rs`);
 *    `use-widget-chat-stream.ts` treats `!response.ok` as a failure and the
 *    assistant message renders in the FAILED state (responseFailed text +
 *    retry button — `message-item.tsx` `isFailed` path). We assert on the
 *    in-widget failed-message area, NOT a toast.
 */

import { test, expect } from './fixtures/chat.fixtures'
import {
  setupWidgetPage,
  createWidgetHelper,
  WIDGET_BASE_URL,
  DEFAULT_WIDGET_CHANNEL_ID,
} from './helpers/widget-helper'

test.describe('US-INTG-006: Widget multi-channel init contract', () => {
  // ---------------------------------------------------------------------------
  // Scenario 1: valid channelId → widget renders and chat returns a response.
  // (US-INTG-006 scn 1 "带 channelId 的 Widget 正常对话".)
  //
  // Full RAG-answer-scoping is already covered API-side (DE-D02) and main-
  // route (DE-D01/DE-D04). Here we assert the widget INIT CONTRACT succeeds
  // for a valid channel: the widget renders + a chat response arrives.
  // ---------------------------------------------------------------------------
  test('US-INTG-006 scenario 1 - valid channelId renders widget and returns a chat response', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: DEFAULT_WIDGET_CHANNEL_ID,
    })

    const helper = createWidgetHelper(page)

    // Assert: the widget host + Shadow DOM chat content rendered (init OK).
    expect(await helper.hasHost()).toBe(true)
    expect(await helper.hasShadowChatContent()).toBe(true)

    // Open the chat modal and send a question.
    await helper.clickFloatingButton()
    await helper.waitForModal()
    expect(await helper.hasChatInput()).toBe(true)

    await helper.sendMessage('What is this knowledge base about?')

    // Wait for an assistant message to appear and streaming to finish.
    await helper.waitForAssistantMessage()
    await helper.waitForStreamingDone()

    // Assert: an assistant response rendered with non-empty content.
    const assistantText = await helper.getLastAssistantText()
    expect(assistantText.length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 2: NO channelId → widget does NOT render; console reports
  // "channelId is required". (US-INTG-006 scn 2 "未传 channelId 时 Widget 报错".)
  //
  // grep pattern for DE-D05: `channelId is required`
  // ---------------------------------------------------------------------------
  test('US-INTG-006 scenario 2 - omitting channelId logs "channelId is required" and does not render', async ({
    page,
  }) => {
    const helper = createWidgetHelper(page)

    // Capture console errors emitted by the widget validation.
    const consoleMessages: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleMessages.push(msg.text())
    })

    // Omit channelId entirely — the legacy failing call shape.
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
    })

    // Assert: the widget host was never created (init returned early), so
    // there is no shadowRoot chat content either.
    expect(await helper.hasHost()).toBe(false)
    expect(await helper.hasShadowChatContent()).toBe(false)

    // Assert: the widget logged the required-channelId error. We match on the
    // substring "channelId is required" (validateWidgetConfig emits
    // `[RWikiChat] channelId is required`). Do NOT assert a toast.
    expect(
      consoleMessages.some((m) => m.includes('channelId is required')),
      `Expected a console error containing "channelId is required". Got: ${JSON.stringify(consoleMessages)}`,
    ).toBe(true)
  })

  // ---------------------------------------------------------------------------
  // Scenario 3: unconfigured channelId → channel unavailable.
  // (US-INTG-006 scn 3 "传入未配置的 channelId 时无法使用 / 频道不存在或不可用".)
  //
  // The widget renders (channelId is a non-empty string) but the FIRST chat
  // request fails with HTTP 400 `频道 channel-unknown 未配置`. The widget
  // surfaces this as a failed assistant message (retry button), in-widget —
  // not a toast. We assert on the in-widget failed-message area.
  // ---------------------------------------------------------------------------
  test('US-INTG-006 scenario 3 - unconfigured channelId surfaces a channel-unavailable error in-widget', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: 'channel-unknown',
    })

    const helper = createWidgetHelper(page)

    // The widget DOES render for a non-empty (but unconfigured) channelId.
    expect(await helper.hasHost()).toBe(true)
    expect(await helper.hasShadowChatContent()).toBe(true)

    // Open the modal and send a question — the channel-unavailable state is
    // only surfaced after the first chat request (server 400).
    await helper.clickFloatingButton()
    await helper.waitForModal()
    await helper.sendMessage('Any question')

    // Assert: the assistant message enters the failed state (the widget's
    // channel-unavailable surface — responseFailed text + retry button).
    await helper.waitForAssistantFailed()

    expect(await helper.isLastAssistantFailed()).toBe(true)

    // The failed bubble renders the localized responseFailed text. We assert
    // the bubble exists and is non-trivial; we do NOT hard-assert a specific
    // server error string (the server message is not surfaced into the DOM —
    // the widget maps any non-OK chat response to a failed bubble).
    const lastAssistantText = await helper.getLastAssistantText()
    expect(lastAssistantText.length).toBeGreaterThan(0)
  })
})
