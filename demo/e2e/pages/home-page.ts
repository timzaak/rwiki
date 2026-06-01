/**
 * Home Page Object — / route
 *
 * Business methods for the home page covering:
 * - US-CORE-006: floating chat button and modal interaction
 *
 * Usage:
 * ```typescript
 * const homePage = new HomePage(page, logger)
 * await homePage.navigate()
 * await homePage.waitForReady()
 * await homePage.openChatModal()
 * ```
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import { FRONTEND_URL } from '../fixtures/chat.fixtures'
import type { UnifiedLogger } from 'playwright-unified-logger'

export class HomePage extends BasePage {
  readonly floatingButton: Locator
  readonly chatModal: Locator
  readonly chatModalHeader: Locator
  readonly chatModalClose: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.floatingButton = page.locator(SELECTORS.chat.floatingButton)
    this.chatModal = page.locator(SELECTORS.chat.modal)
    this.chatModalHeader = page.locator(SELECTORS.chat.modalHeader)
    this.chatModalClose = page.locator(SELECTORS.chat.modalClose)
  }

  /**
   * Navigate to the home page using FRONTEND_URL (Vite dev server).
   * Do NOT use BasePage.goto() — it prefixes with BASE_URL (port 8080).
   */
  async navigate(): Promise<void> {
    await this.page.goto(`${FRONTEND_URL}/`)
  }

  /**
   * Verify the home page loaded — check for the heading.
   */
  async waitForReady(): Promise<void> {
    await expect(this.page.locator('h1')).toBeVisible()
  }

  /**
   * Click the floating chat button.
   */
  async clickFloatingButton(): Promise<void> {
    await this.smartClick(this.floatingButton)
  }

  /**
   * Open the chat modal by clicking the floating button and waiting for the modal.
   */
  async openChatModal(): Promise<void> {
    await this.clickFloatingButton()
    await expect(this.chatModal).toBeVisible()
    await expect(this.chatModalHeader).toBeVisible()
  }

  /**
   * Close the chat modal by clicking the close button.
   */
  async closeChatModal(): Promise<void> {
    await this.smartClick(this.chatModalClose)
    await expect(this.chatModal).toBeHidden()
  }

  /**
   * Check if the chat modal is currently visible.
   */
  async isChatModalOpen(): Promise<boolean> {
    return await this.isVisible(this.chatModal)
  }

  /**
   * Check if the floating button is currently visible.
   */
  async isFloatingButtonVisible(): Promise<boolean> {
    return await this.isVisible(this.floatingButton)
  }
}
