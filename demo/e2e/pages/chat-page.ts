/**
 * Chat Page Object — floating chat modal
 *
 * Business methods for the chat modal (opened via floating button) covering:
 * - US-CORE-002: multi-turn conversation
 * - US-CORE-003: streaming response
 * - US-CORE-005: chat modal layout
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import { FRONTEND_URL } from '../fixtures/chat.fixtures'
import type { UnifiedLogger } from 'playwright-unified-logger'

export class ChatPage extends BasePage {
  // Chat modal and scoped locators
  readonly modal: Locator
  readonly panel: Locator
  readonly input: Locator
  readonly sendButton: Locator
  readonly messageList: Locator
  readonly messageListEmpty: Locator
  readonly errorBanner: Locator
  readonly suggestedQuestionsContainer: Locator
  readonly suggestedQuestionButtons: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.modal = page.locator(SELECTORS.chat.modal)
    this.panel = this.modal.locator(SELECTORS.chat.panel)
    this.input = this.modal.locator(SELECTORS.chat.input)
    this.sendButton = this.modal.locator(SELECTORS.chat.sendButton)
    this.messageList = this.modal.locator(SELECTORS.chat.messageList)
    this.messageListEmpty = this.modal.locator(SELECTORS.chat.messageListEmpty)
    this.errorBanner = this.modal.locator(SELECTORS.chat.errorBanner)
    this.suggestedQuestionsContainer = this.modal.locator(SELECTORS.chat.suggestedQuestions)
    this.suggestedQuestionButtons = this.modal.locator(SELECTORS.chat.suggestedQuestionButton)
  }

  /**
   * Navigate to the home page and open the chat modal via floating button.
   */
  async navigate(): Promise<void> {
    await this.page.goto(`${FRONTEND_URL}/`)
    const floatingBtn = this.page.locator(SELECTORS.chat.floatingButton)
    await expect(floatingBtn).toBeVisible()
    await floatingBtn.click()
    await expect(this.modal).toBeVisible()
  }

  /**
   * Verify the chat page is ready: panel and input visible.
   */
  async waitForReady(): Promise<void> {
    await expect(this.panel).toBeVisible()
    await expect(this.input).toBeVisible()
  }

  /**
   * Fill the chat input and click send.
   */
  async sendMessage(text: string): Promise<void> {
    await this.fillField(this.input, text)
    await this.smartClick(this.sendButton)
  }

  /**
   * Wait for an assistant response to appear and streaming to finish.
   *
   * Streaming is done when no message-item-streaming element is present.
   */
  async waitForAssistantResponse(timeout: number = 30000): Promise<void> {
    // Wait for at least one assistant message
    const assistantLocator = this.modal.locator(SELECTORS.chat.messageItem('assistant'))
    await expect(assistantLocator.last()).toBeVisible({ timeout })

    // Wait for streaming indicator to disappear
    const streamingIndicator = this.modal.locator(SELECTORS.chat.messageStreaming)
    await expect(streamingIndicator).toHaveCount(0, { timeout })
  }

  /**
   * Get all assistant message text contents.
   */
  async getAssistantMessages(): Promise<string[]> {
    const messages = this.modal.locator(SELECTORS.chat.messageItem('assistant'))
    const count = await messages.count()
    const contents: string[] = []
    for (let i = 0; i < count; i++) {
      const text = await messages.nth(i).textContent()
      contents.push(text ?? '')
    }
    return contents
  }

  /**
   * Get all user message text contents.
   */
  async getUserMessages(): Promise<string[]> {
    const messages = this.modal.locator(SELECTORS.chat.messageItem('user'))
    const count = await messages.count()
    const contents: string[] = []
    for (let i = 0; i < count; i++) {
      const text = await messages.nth(i).textContent()
      contents.push(text ?? '')
    }
    return contents
  }

  // --- Suggested questions (pre-question suggestions) ---

  /**
   * Get all visible suggested question button texts.
   * Returns empty array if container is not visible.
   */
  async getSuggestedQuestions(): Promise<string[]> {
    const visible = await this.suggestedQuestionsContainer.isVisible().catch(() => false)
    if (!visible) return []

    const count = await this.suggestedQuestionButtons.count()
    const texts: string[] = []
    for (let i = 0; i < count; i++) {
      const text = await this.suggestedQuestionButtons.nth(i).textContent()
      texts.push(text ?? '')
    }
    return texts
  }

  /**
   * Get a locator for the suggested question button containing the given text.
   */
  getSuggestedQuestionButtonByText(text: string): Locator {
    return this.suggestedQuestionButtons.filter({ hasText: text })
  }

  /**
   * Click a suggested question button matching the text.
   * After click, the button disappears because messages.length > 0.
   */
  async clickSuggestedQuestion(text: string): Promise<void> {
    const button = this.getSuggestedQuestionButtonByText(text)
    await this.smartClick(button)
  }

  /**
   * Wait for the suggested-questions container to be visible.
   */
  async waitForSuggestedQuestions(timeout: number = 10000): Promise<void> {
    await expect(this.suggestedQuestionsContainer).toBeVisible({ timeout })
  }

  /**
   * Wait for the suggested-questions container to be absent/hidden.
   * The component returns null when empty, so the element detaches from DOM.
   */
  async waitForNoSuggestions(timeout: number = 10000): Promise<void> {
    await expect(this.suggestedQuestionsContainer).toBeHidden({ timeout })
  }

}
