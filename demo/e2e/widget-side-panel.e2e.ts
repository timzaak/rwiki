/* global window, document */
/**
 * Widget Side-Panel E2E Tests — US-INTG-018 / US-CORE-047
 *
 * Covers the side-panel display mode (`displayMode: 'sidePanel'`) of the
 * standalone widget bundle: host push, interaction persistence, host cleanup,
 * ask() routing, mobile fullscreen fallback, invalid config fail-fast, and
 * the modal default regression.
 *
 * User stories: US-INTG-018 为 Widget 选择显示形态 (scenarios 1-5) and
 * US-CORE-047 在右侧停靠侧栏中使用聊天 (scenarios 1-3),
 * docs/user-stories/.
 *
 * Boundary: tests target the standalone IIFE widget bundle
 * (`${BASE_URL}/widget/rwiki-chat.js`) loaded via `page.setContent` into a
 * Shadow DOM, optionally with host in-flow reference content (HOST_BODY).
 * Do NOT use ChatPage. Do NOT modify widget source.
 *
 * IMPORTANT: the widget mounts inside a Shadow DOM. Playwright locator()
 * does NOT pierce Shadow DOM, so widget queries go through page.evaluate()
 * via helpers/widget-helper. Light-DOM host content (#host-content,
 * #host-button) is queried normally.
 */

import { test, expect } from './fixtures/chat.fixtures'
import {
  setupWidgetPage,
  createWidgetHelper,
  WIDGET_BASE_URL,
  DEFAULT_WIDGET_CHANNEL_ID,
} from './helpers/widget-helper'

/**
 * Host in-flow reference content: a full-width <main> with a clickable
 * button. The button records clicks on window so tests can assert the host
 * content stays interactive while the panel is open (push, not overlay).
 */
const HOST_BODY = `
  <main id="host-content" style="width: 100%; padding: 16px;">
    <h1>Host page article</h1>
    <p>Host in-flow content that the side panel pushes left when it opens.</p>
    <button id="host-button" type="button">Host action</button>
  </main>
  <script>
    document.getElementById('host-button').addEventListener('click', function () {
      window.__hostClicked = (window.__hostClicked || 0) + 1
    })
  </script>
`

test.describe('US-INTG-018 / US-CORE-047: Widget side-panel display mode', () => {
  // US-INTG-018 scenario 2 / US-CORE-047 scenario 1: floating button opens a
  // full-height docked panel; the host document flow is pushed left by the
  // panel width; the host stays interactive; the floating button hides while
  // the panel is open.
  test('US-INTG-018 scn 2 / US-CORE-047 scn 1 - side panel pushes host content and stays interactive', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'sidePanel',
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)

    await helper.waitForFloatingButton()
    const widthBefore = await helper.getElementWidth('#host-content')
    expect(widthBefore).toBeGreaterThan(0)

    // Open the side panel via the floating button, then wait for the push
    // transition (300ms ease-out) to settle before measuring.
    await helper.clickFloatingButton()
    await helper.waitForSidePanel()
    await helper.waitForHostPushApplied()

    // The push margin and the panel width share --rwiki-side-panel-width:
    // they must be equal (single width source, no drift).
    const panelWidth = await helper.getSidePanelWidth()
    const marginRight = await helper.getHtmlMarginRight()
    expect(panelWidth).toBeGreaterThan(300)
    expect(Math.abs(marginRight - panelWidth)).toBeLessThanOrEqual(1)

    // The host in-flow reference element narrowed by the pushed amount.
    const widthWhileOpen = await helper.getElementWidth('#host-content')
    expect(Math.abs(widthBefore - widthWhileOpen - marginRight)).toBeLessThanOrEqual(
      1,
    )

    // While open: floating button hidden (auto-hide guard; it unmounts via
    // conditional rendering). Asserted atomically with the panel still
    // mounted so the negative check cannot pass vacuously if the whole
    // widget tree disappears mid-test.
    const buttonHiddenWhilePanelOpen = await page.evaluate(() => {
      const root = document.getElementById('rwiki-chat-widget')?.shadowRoot
      return (
        root?.querySelector('[data-testid="chat-side-panel"]') != null &&
        root?.querySelector('[data-testid="floating-chat-button"]') == null
      )
    })
    expect(buttonHiddenWhilePanelOpen).toBe(true)

    // While open: host content remains interactive (push, not overlay).
    await page.click('#host-button')
    expect(await page.evaluate(() => (window as any).__hostClicked)).toBe(1)
  })

  // US-CORE-047 scenario 2: conversation survives close + reopen (session
  // state lives in the store, not in the panel shell).
  test('US-CORE-047 scn 2 - conversation is preserved across close and reopen', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: WIDGET_BASE_URL,
      channelId: DEFAULT_WIDGET_CHANNEL_ID,
      displayMode: 'sidePanel',
    })

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()

    await helper.clickFloatingButton()
    await helper.waitForSidePanel()
    await helper.sendMessage('What is this knowledge base about?')
    await helper.waitForAssistantMessage()
    await helper.waitForStreamingDone()
    const textBefore = await helper.getLastAssistantText()
    expect(textBefore.length).toBeGreaterThan(0)

    await helper.clickSidePanelClose()
    await helper.waitForSidePanelHostCleanup()
    await helper.waitForFloatingButton()
    await helper.clickFloatingButton()
    await helper.waitForSidePanel()

    expect(await helper.hasUserMessage()).toBe(true)
    expect(await helper.getLastAssistantText()).toBe(textBefore)
  })

  // US-INTG-018 scenario 3: close restores the host layout with zero
  // residue; rapid toggling stays idempotent; destroy() cleans up
  // synchronously even while the panel is open.
  test('US-INTG-018 scn 3 - close, rapid toggling and destroy leave no host residue', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'sidePanel',
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()
    const widthBefore = await helper.getElementWidth('#host-content')

    // Plain close: class off immediately, style removed after the 300ms
    // slide-out window, host width restored.
    await helper.clickFloatingButton()
    await helper.waitForSidePanel()
    await helper.waitForHostPushApplied()
    await helper.clickSidePanelClose()
    await helper.waitForSidePanelHostCleanup()
    expect(await helper.hasSidePanelHostStyle()).toBe(false)
    expect(await helper.hasSidePanelOpenClass()).toBe(false)
    expect(Math.abs((await helper.getElementWidth('#host-content')) - widthBefore)).toBeLessThanOrEqual(
      1,
    )

    // Rapid open/close cycle: reopen cancels the delayed style removal; the
    // final close must still converge to a fully clean host.
    for (let i = 0; i < 3; i++) {
      await helper.waitForFloatingButton()
      await helper.clickFloatingButton()
      await helper.waitForSidePanel()
      await helper.clickSidePanelClose()
    }
    await helper.waitForSidePanelHostCleanup()
    expect(await helper.hasSidePanelHostStyle()).toBe(false)
    expect(await helper.hasSidePanelOpenClass()).toBe(false)
    expect(Math.abs((await helper.getElementWidth('#host-content')) - widthBefore)).toBeLessThanOrEqual(
      1,
    )

    // destroy() while the panel is open: synchronous full cleanup — the
    // widget host element AND every host-side injection are gone immediately.
    await helper.waitForFloatingButton()
    await helper.clickFloatingButton()
    await helper.waitForSidePanel()
    await page.evaluate(() => {
      ;(window as any).RWikiChat.destroy()
    })
    expect(await helper.hasHost()).toBe(false)
    expect(await helper.hasSidePanelHostStyle()).toBe(false)
    expect(await helper.hasSidePanelOpenClass()).toBe(false)
    expect(Math.abs((await helper.getElementWidth('#host-content')) - widthBefore)).toBeLessThanOrEqual(
      1,
    )
  })

  // US-INTG-018 scenario 4: ask() auto-routes to the side-panel form — it
  // opens the panel (with host push) and sends the question as one user turn.
  test('US-INTG-018 scn 4 - ask() opens the side panel and auto-sends the question', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'sidePanel',
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()

    // No manual open — ask() alone must open the panel and push the host.
    await page.evaluate(() => {
      ;(window as any).RWikiChat.ask('What is this knowledge base about?')
    })
    await helper.waitForSidePanel()
    await helper.waitForHostPushApplied()

    expect(await helper.hasUserMessage()).toBe(true)
    await helper.waitForAssistantMessage()
    await helper.waitForStreamingDone()
    expect((await helper.getLastAssistantText()).length).toBeGreaterThan(0)
  })

  // US-CORE-047 scenario 3 (mobile): below the 640px breakpoint the panel
  // covers the page fullscreen and the host is never pushed.
  test('US-CORE-047 scn 3 - 375px viewport: fullscreen panel, host not pushed', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.setViewportSize({ width: 375, height: 667 })
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'sidePanel',
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()

    await helper.clickFloatingButton()
    await helper.waitForSidePanel()

    // Fullscreen: panel spans the viewport; push margin stays 0 (the push
    // media query `min-width: 40rem` never matches below 640px). Fractional
    // device pixel ratios can report 375 - 1/32768 — allow subpixel slack.
    expect(Math.abs((await helper.getSidePanelWidth()) - 375)).toBeLessThanOrEqual(1)
    expect(await helper.getHtmlMarginRight()).toBe(0)

    await helper.clickSidePanelClose()
    await helper.waitForSidePanelHostCleanup()
    expect(await helper.getHtmlMarginRight()).toBe(0)
  })

  // US-CORE-047 scenario 3 (boundary, >= 640px): at 700px — inside the
  // [640, 768) band — the panel must be DOCKED and the host PUSHED. This
  // locks the breakpoint alignment between the push media query
  // (`min-width: 40rem` = 640px) and the panel fullscreen fallback
  // (`max-sm` = 640px): a misaligned pair would dock the panel WITHOUT
  // pushing, covering host content.
  test('US-CORE-047 scn 3 boundary - 700px viewport: docked panel and pushed host', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.setViewportSize({ width: 700, height: 800 })
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'sidePanel',
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()

    await helper.clickFloatingButton()
    await helper.waitForSidePanel()
    await helper.waitForHostPushApplied()

    // Docked (not fullscreen): panel width = min(420px, 75vw) = 420px here,
    // strictly narrower than the viewport.
    const panelWidth = await helper.getSidePanelWidth()
    expect(panelWidth).toBeGreaterThan(300)
    expect(panelWidth).toBeLessThan(700)

    // Pushed: margin equals the panel width (shared width variable).
    const marginRight = await helper.getHtmlMarginRight()
    expect(Math.abs(marginRight - panelWidth)).toBeLessThanOrEqual(1)

    await helper.clickSidePanelClose()
    await helper.waitForSidePanelHostCleanup()
    expect(await helper.getHtmlMarginRight()).toBe(0)
  })

  // US-INTG-018 scenario 5: unsupported displayMode value → console error
  // and no widget host at all (fail fast, no silent modal fallback).
  test('US-INTG-018 scn 5 - invalid displayMode logs an error and does not render', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const helper = createWidgetHelper(page)

    const consoleMessages: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleMessages.push(msg.text())
    })

    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
        displayMode: 'drawer',
      },
      HOST_BODY,
    )

    expect(await helper.hasHost()).toBe(false)

    expect(
      consoleMessages.some((m) => m.includes('displayMode must be')),
      `Expected a console error containing "displayMode must be". Got: ${JSON.stringify(consoleMessages)}`,
    ).toBe(true)
  })

  // US-INTG-018 scenario 1 (regression): omitting displayMode keeps the
  // modal form and leaves the host layout completely untouched.
  test('US-INTG-018 scn 1 - omitted displayMode keeps the modal and never touches the host layout', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await setupWidgetPage(
      page,
      {
        apiUrl: WIDGET_BASE_URL,
        channelId: DEFAULT_WIDGET_CHANNEL_ID,
      },
      HOST_BODY,
    )

    const helper = createWidgetHelper(page)
    await helper.waitForFloatingButton()
    const widthBefore = await helper.getElementWidth('#host-content')

    // Default form is the modal (zero-change compatibility).
    await helper.clickFloatingButton()
    await helper.waitForModal()
    expect(await helper.hasChatInput()).toBe(true)

    // Modal mode never injects host-side push styles.
    expect(await helper.hasSidePanelHostStyle()).toBe(false)
    expect(await helper.getHtmlMarginRight()).toBe(0)
    expect(
      Math.abs((await helper.getElementWidth('#host-content')) - widthBefore),
    ).toBeLessThanOrEqual(1)
  })
})
