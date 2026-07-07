/**
 * 频道聊天浮窗 E2E（support-multiple-website 迁移）
 *
 * Main-channel chat now lives under `/c/$channelId`; root `/` is a channel-entry list
 * (no floating button). The modal is opened from `/c/help_center` via the
 * floating button, which only renders once the route validates the channel.
 *
 * US-story traceability:
 * - US-INTG-007 (DRAFT, `.ai/user-stories/integration/support-multiple-website.md`)
 *   - Scenario 1 "通过 /c/channel-a 访问频道": open/close the floating chat modal
 *     on `/c/help_center`, and verify the conversation is preserved across a
 *     close/reopen cycle. (Scenario 2 — unknown channel — is covered by DE-D04.)
 *
 * Dependencies:
 * - demo/e2e/fixtures/chat.fixtures.ts (demoLogger via fixture)
 * - demo/e2e/pages/chat-page.ts (navigate('/c/$channelId') + open modal)
 * - demo/e2e/selectors.ts (SELECTORS.chat.*)
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'
import { SELECTORS } from './selectors'

test.describe('Chat Modal on /c/help_center', () => {
  // US-INTG-007 - Scenario 1a: open and close floating chat modal from a channel route
  test('US-INTG-007 scenario 1 - open floating chat modal from /c/help_center', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    // navigate() waits for the channel to resolve (floating button visible = ready)
    // and opens the modal.
    await chatPage.navigate('help_center')

    // Assert: chat-modal is visible
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible()

    // Assert: chat-modal-header is visible (stable element, not auto-dismissing)
    await expect(page.locator(SELECTORS.chat.modalHeader)).toBeVisible()

    // Assert: chat-modal-close button is visible
    await expect(page.locator(SELECTORS.chat.modalClose)).toBeVisible()

    // Assert: chat-panel is visible inside the modal
    await expect(page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.panel)).toBeVisible()

    // Assert: chat-input is visible inside the modal
    await expect(page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.input)).toBeVisible()

    // Assert: floating-chat-button is hidden while the modal is open
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeHidden()
  })

  // US-INTG-007 - Scenario 1b: close and reopen modal preserves conversation on /c/help_center
  test('US-INTG-007 scenario 1 - close and reopen modal preserves conversation', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate('help_center')

    // Send a message via the modal's chat input (channel-scoped, channelId carried by route)
    const chatInput = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.input)
    const sendButton = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.sendButton)
    await chatInput.fill('Hello test message')
    await sendButton.click()

    // Wait for the user message to appear inside the modal
    const userMessage = page
      .locator(SELECTORS.chat.modal)
      .locator(SELECTORS.chat.messageItem('user'))
    await expect(userMessage).toBeVisible({ timeout: 5000 })
    expect(await userMessage.count()).toBeGreaterThanOrEqual(1)

    // Close the modal via the close button
    await page.locator(SELECTORS.chat.modalClose).click()

    // Assert: chat-modal is hidden
    await expect(page.locator(SELECTORS.chat.modal)).toBeHidden()

    // Assert: floating-chat-button is visible again (channel still resolved)
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeVisible()

    // Reopen the modal by clicking the floating button
    await page.locator(SELECTORS.chat.floatingButton).click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible()

    // Assert: the previous user message is still visible (conversation preserved)
    const preservedUserMessage = page
      .locator(SELECTORS.chat.modal)
      .locator(SELECTORS.chat.messageItem('user'))
    await expect(preservedUserMessage).toBeVisible()
    expect(await preservedUserMessage.count()).toBeGreaterThanOrEqual(1)

    // Assert: the user message content is still "Hello test message"
    const preservedText = await preservedUserMessage.first().textContent()
    expect(preservedText).toContain('Hello test message')
  })
})
