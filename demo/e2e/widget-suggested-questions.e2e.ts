/* global window, document */
/**
 * Widget Suggested Questions E2E Tests
 *
 * Covers US-INTG-004: Configure suggested questions in Widget.
 * Tests target the standalone widget bundle which resolves suggestions
 * client-side via matchSuggestedQuestions(config, navigator.language) -- no API call.
 *
 * IMPORTANT: The widget mounts inside a Shadow DOM. Playwright locator()
 * does NOT pierce Shadow DOM, so all queries use page.evaluate() to
 * access elements inside the shadow root.
 *
 * Do NOT import or use ChatPage in this file.
 *
 * Scenarios:
 * 1. Widget with locale map shows matched language buttons
 * 2. Widget with simple array shows those buttons
 * 3. Widget without suggestedQuestions shows no buttons
 */

import type { Page } from '@playwright/test'
import { test, expect } from './fixtures/chat.fixtures'

const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'

/**
 * Load the built widget IIFE bundle into a fresh page and initialize it.
 * Each test calls this to get an isolated widget instance.
 */
async function setupWidgetPage(page: Page, config: Record<string, unknown>) {
  await page.setContent(`
    <!DOCTYPE html><html><body>
    <script src="${BASE_URL}/widget/rwiki-chat.js"></script>
    </body></html>
  `)
  await page.evaluate((cfg) => {
    ;(window as any).RWikiChat.init(cfg)
  }, config)
}

/**
 * Helper for querying inside the widget Shadow DOM.
 * All methods use page.evaluate() since Playwright locators
 * cannot pierce Shadow DOM boundaries.
 */
const createWidgetHelper = (page: Page) => ({
  async getQuestionTexts(): Promise<string[]> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const buttons = root?.querySelectorAll(
        '[data-testid="suggested-question-button"]'
      )
      return Array.from(buttons || []).map((b) => b.textContent || '')
    })
  },

  async hasQuestionButtons(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return (
        (root?.querySelectorAll('[data-testid="suggested-question-button"]')
          .length || 0) > 0
      )
    })
  },

  async clickFloatingButton(): Promise<void> {
    await page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const btn = root?.querySelector(
        '[data-testid="floating-chat-button"]'
      ) as HTMLElement
      btn?.click()
    })
  },

  async isModalOpen(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const modal = root?.querySelector('[data-testid="chat-modal"]')
      return modal !== null && modal !== undefined
    })
  },

  async hasChatInput(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-input"]') !== null
    })
  },
})

test.describe('US-INTG-004: Widget suggested questions', () => {
  // ---------------------------------------------------------------------------
  // Scenario 1: Widget with locale map shows matched language buttons
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 1 - widget with locale map shows matched language buttons', async ({
    page,
  }) => {
    // Playwright default locale is en-US; "en" is a prefix match.
    await setupWidgetPage(page, {
      apiUrl: BASE_URL,
      suggestedQuestions: {
        default: ['Default Q'],
        en: ['English Q1', 'English Q2'],
      },
    })

    const helper = createWidgetHelper(page)

    // Open the chat modal by clicking the floating button inside Shadow DOM
    await helper.clickFloatingButton()

    // Wait for the modal to render
    await page.waitForFunction(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-modal"]') !== null
    })

    // Assert: modal is open
    expect(await helper.isModalOpen()).toBe(true)

    // Wait for suggested question buttons to appear (React needs a render cycle)
    await page.waitForFunction(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return (
        (root?.querySelectorAll('[data-testid="suggested-question-button"]')
          .length || 0) > 0
      )
    })

    // Assert: question buttons match the "en" locale (prefix of en-US)
    const texts = await helper.getQuestionTexts()
    expect(texts).toContain('English Q1')
    expect(texts).toContain('English Q2')

    // Assert: default question is NOT shown (en matched, not default)
    expect(texts).not.toContain('Default Q')
  })

  // ---------------------------------------------------------------------------
  // Scenario 2: Widget with simple array shows those buttons
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 2 - widget with simple array shows those buttons', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: BASE_URL,
      suggestedQuestions: ['Question A', 'Question B'],
    })

    const helper = createWidgetHelper(page)

    // Open the chat modal
    await helper.clickFloatingButton()

    // Wait for the modal to render
    await page.waitForFunction(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-modal"]') !== null
    })

    // Wait for suggested question buttons to appear
    await page.waitForFunction(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return (
        (root?.querySelectorAll('[data-testid="suggested-question-button"]')
          .length || 0) > 0
      )
    })

    // Assert: buttons match the simple array (passed through directly)
    const texts = await helper.getQuestionTexts()
    expect(texts).toContain('Question A')
    expect(texts).toContain('Question B')
  })

  // ---------------------------------------------------------------------------
  // Scenario 3: Widget without suggestedQuestions shows no buttons
  // ---------------------------------------------------------------------------
  test('US-INTG-004 scenario 3 - widget without suggestedQuestions shows no buttons', async ({
    page,
  }) => {
    await setupWidgetPage(page, {
      apiUrl: BASE_URL,
    })

    const helper = createWidgetHelper(page)

    // Open the chat modal
    await helper.clickFloatingButton()

    // Wait for the modal to render
    await page.waitForFunction(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-modal"]') !== null
    })

    // Assert: modal is open
    expect(await helper.isModalOpen()).toBe(true)

    // Assert: no suggested question buttons exist
    expect(await helper.hasQuestionButtons()).toBe(false)

    // Assert: chat input is present and functional (widget works normally)
    expect(await helper.hasChatInput()).toBe(true)
  })
})
