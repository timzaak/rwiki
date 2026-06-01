/**
 * Chat Floating Modal E2E Tests
 *
 * Covers:
 * - US-CORE-006: Click floating button on home page -> verify modal opens ->
 *   close -> reopen -> verify conversation preserved
 *   - Scenario 1: Opening and closing the chat modal
 *   - Scenario 2: Conversation preserved after close/reopen
 *   - Additional: Floating button hidden on /chat route
 *
 * Dependencies (from DE-D01):
 * - demo/e2e/fixtures/chat.fixtures.ts
 * - demo/e2e/pages/home-page.ts
 * - demo/e2e/selectors.ts
 */

import { test, expect } from './fixtures/chat.fixtures'
import { HomePage } from './pages/home-page'
import { ChatPage } from './pages/chat-page'
import { SELECTORS } from './selectors'

// ---------------------------------------------------------------------------
// US-CORE-006 - Scenario 1: Open and close floating chat modal from home page
// ---------------------------------------------------------------------------
test.describe('Chat Modal', () => {
  test('US-CORE-006 scenario 1 - open floating chat modal from home page', async ({ page }) => {
    const homePage = new HomePage(page)
    await homePage.navigate()
    await homePage.waitForReady()

    // Assert: floating-chat-button is visible on the home page
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeVisible()

    // Click the floating button to open the modal
    await homePage.openChatModal()

    // Assert: chat-modal is visible
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible()

    // Assert: chat-modal-header is visible
    await expect(page.locator(SELECTORS.chat.modalHeader)).toBeVisible()

    // Assert: chat-modal-header contains "Chat Assistant" text
    await expect(page.locator(SELECTORS.chat.modalHeader)).toContainText('Chat Assistant')

    // Assert: chat-modal-close button is visible
    await expect(page.locator(SELECTORS.chat.modalClose)).toBeVisible()

    // Assert: chat-panel is visible inside the modal (ChatPanel is reused)
    await expect(page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.panel)).toBeVisible()

    // Assert: chat-input is visible inside the modal
    await expect(page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.input)).toBeVisible()

    // Assert: floating-chat-button is now hidden (floating button hides when modal is open)
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeHidden()
  })

  // US-CORE-006 - Scenario 2: Close and reopen modal preserves conversation
  test('US-CORE-006 scenario 2 - close and reopen modal preserves conversation', async ({
    page,
  }) => {
    const homePage = new HomePage(page)
    await homePage.navigate()
    await homePage.waitForReady()

    // Open the chat modal
    await homePage.openChatModal()

    // Send a message via the modal's chat input
    const chatInput = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.input)
    const sendButton = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.sendButton)
    await chatInput.fill('Hello test message')
    await sendButton.click()

    // Wait for the user message to appear in the message list inside the modal
    const userMessage = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.messageItem('user'))
    await expect(userMessage).toBeVisible({ timeout: 5000 })

    // Assert: at least one user message is visible inside the modal
    const userMessageCount = await userMessage.count()
    expect(userMessageCount).toBeGreaterThanOrEqual(1)

    // Close the modal via the close button
    await homePage.closeChatModal()

    // Assert: chat-modal is hidden
    await expect(page.locator(SELECTORS.chat.modal)).toBeHidden()

    // Assert: floating-chat-button is visible again
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeVisible()

    // Reopen the modal by clicking the floating button
    await homePage.openChatModal()

    // Assert: chat-modal is visible again
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible()

    // Assert: the previous user message is still visible (conversation preserved)
    const preservedUserMessage = page.locator(SELECTORS.chat.modal).locator(SELECTORS.chat.messageItem('user'))
    await expect(preservedUserMessage).toBeVisible()
    const preservedCount = await preservedUserMessage.count()
    expect(preservedCount).toBeGreaterThanOrEqual(1)

    // Assert: the user message content is still "Hello test message"
    const preservedText = await preservedUserMessage.first().textContent()
    expect(preservedText).toContain('Hello test message')
  })

  // Additional test: Floating button is hidden on /chat route
  test('US-CORE-006 - floating button is hidden on /chat route', async ({ page }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate()
    await chatPage.waitForReady()

    // Assert: floating-chat-button is NOT visible on the dedicated chat page
    await expect(page.locator(SELECTORS.chat.floatingButton)).toBeHidden()
  })
})
