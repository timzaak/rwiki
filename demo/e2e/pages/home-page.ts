/**
 * Home Page Object — `/` channel-entry list
 *
 * With multi-channel support, root `/` is a channel discovery page (a list of
 * configured channel entries), NOT a chat surface. Chat lives under `/c/$channelId`.
 *
 * Covers:
 * - US-INTG-005: channel discovery — root `/` lists configured channels and links to
 *   `/c/$channelId` for each (`channel-entry-${id}`).
 *
 * Usage:
 * ```typescript
 * const homePage = new HomePage(page, demoLogger)
 * await homePage.navigate()
 * await homePage.waitForChannelEntries()
 * await homePage.clickChannelEntry('help_center')
 * ```
 */

import { Page, Locator, expect } from '@playwright/test'
import { SELECTORS } from '../selectors'
import { BasePage } from './base-page'
import { FRONTEND_URL } from '../fixtures/chat.fixtures'
import type { UnifiedLogger } from 'playwright-unified-logger'

export class HomePage extends BasePage {
  readonly channelEntryListLoading: Locator
  readonly channelEntryListError: Locator
  readonly channelEntryListEmpty: Locator
  readonly channelEntry: Locator

  constructor(page: Page, logger?: UnifiedLogger) {
    super(page, logger)
    this.channelEntryListLoading = page.locator(SELECTORS.channel.channelListLoading)
    this.channelEntryListError = page.locator(SELECTORS.channel.channelListError)
    this.channelEntryListEmpty = page.locator(SELECTORS.channel.channelListEmpty)
    this.channelEntry = page.locator(SELECTORS.channel.channelEntry)
  }

  /**
   * Navigate to the home page (channel-entry list) using FRONTEND_URL (Vite dev server).
   * Do NOT use BasePage.goto() — it prefixes with BASE_URL (backend port 18080).
   */
  async navigate(): Promise<void> {
    await this.page.goto(`${FRONTEND_URL}/`)
  }

  /**
   * Wait for the channel-entry list to finish loading and render at least one entry.
   * Asserts the loading placeholder is gone and a `channel-entry` is visible.
   */
  async waitForChannelEntries(timeout: number = 10000): Promise<void> {
    // The list briefly shows channel-list-loading; ready state renders channel-entry.
    await expect(this.channelEntryListLoading).toBeHidden({ timeout })
    await expect(this.channelEntry.first()).toBeVisible({ timeout })
  }

  /**
   * Get a locator for the channel-entry link for the given channel id.
   */
  channelEntryById(id: string): Locator {
    return this.page.locator(SELECTORS.channel.channelEntryById(id))
  }

  /**
   * Click the channel-entry link for the given channel id (navigates to `/c/$channelId`).
   */
  async clickChannelEntry(id: string): Promise<void> {
    const entry = this.channelEntryById(id)
    await this.smartClick(entry)
  }
}
