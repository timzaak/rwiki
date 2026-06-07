/**
 * Suggested Questions E2E Tests
 *
 * Covers US-CORE-028: Through suggested question buttons quickly start conversation.
 *
 * Scenarios:
 * 1. Empty state shows suggested question buttons when configured
 * 2. API locale matching contract (zh-CN / fallback)
 * 3. No locale match falls back to default
 * 4. Click suggested question sends message and buttons disappear
 * 5. Not configured shows no suggestion buttons
 * 6. Manual input makes suggested question buttons disappear
 *
 * Dependencies (from DE-D01):
 * - demo/e2e/fixtures/chat.fixtures.ts
 * - demo/e2e/pages/chat-page.ts
 * - demo/e2e/selectors.ts
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'

const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'

test.describe('US-CORE-028: Suggested questions', () => {
  let chatPage: ChatPage

  test.beforeEach(async ({ page, demoLogger }) => {
    chatPage = new ChatPage(page, demoLogger)
  })

  // ---------------------------------------------------------------------------
  // Scenario 1: Empty state shows suggested question buttons when configured
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 1 - empty state shows suggested question buttons when configured', async () => {
    await chatPage.navigate()
    await chatPage.waitForReady()

    // Wait for suggested questions to load from API
    await chatPage.waitForSuggestedQuestions()

    // Assert: suggestions container is visible
    await expect(chatPage.suggestedQuestionsContainer).toBeVisible()

    // Assert: at least one suggestion button is visible
    const buttonCount = await chatPage.suggestedQuestionButtons.count()
    expect(buttonCount).toBeGreaterThan(0)

    // Assert: all button texts are non-empty strings
    const texts = await chatPage.getSuggestedQuestions()
    expect(texts.length).toBeGreaterThan(0)
    for (const text of texts) {
      expect(text.length).toBeGreaterThan(0)
    }
  })

  // ---------------------------------------------------------------------------
  // Scenario 2: API locale matching contract (zh-CN / fallback)
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 2 - API locale matching returns correct language questions', async ({
    page,
  }) => {
    // Verify zh-CN returns Chinese questions
    const zhResponse = await page.request.get(`${BASE_URL}/api/chat/suggestions?locale=zh-CN`)
    expect(zhResponse.status()).toBe(200)
    const zhBody = await zhResponse.json()
    expect(zhBody.questions).toBeInstanceOf(Array)
    expect(zhBody.questions.length).toBeGreaterThan(0)
    // At least one question should contain Chinese characters
    const hasChinese = zhBody.questions.some((q: string) => /[一-鿿]/.test(q))
    expect(hasChinese).toBeTruthy()

    // Verify en-US returns English questions
    const enResponse = await page.request.get(`${BASE_URL}/api/chat/suggestions?locale=en-US`)
    expect(enResponse.status()).toBe(200)
    const enBody = await enResponse.json()
    expect(enBody.questions).toBeInstanceOf(Array)
    expect(enBody.questions.length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 3: No locale match falls back to default
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 3 - no locale match falls back to default questions', async ({
    page,
  }) => {
    // French is not configured, should fall back to default group
    const response = await page.request.get(`${BASE_URL}/api/chat/suggestions?locale=fr`)
    expect(response.status()).toBe(200)
    const body = await response.json()
    expect(body.questions).toBeInstanceOf(Array)
    // Default group is configured, so questions should not be empty
    expect(body.questions.length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 4: Click suggested question sends message and buttons disappear
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 4 - click suggested question sends message and buttons disappear', async () => {
    await chatPage.navigate()
    await chatPage.waitForReady()
    await chatPage.waitForSuggestedQuestions()

    // Get the first suggested question text
    const questions = await chatPage.getSuggestedQuestions()
    expect(questions.length).toBeGreaterThan(0)
    const firstQuestion = questions[0]

    // Click the suggested question button
    await chatPage.clickSuggestedQuestion(firstQuestion)

    // Assert: user message appears with the clicked question text
    const userMessages = await chatPage.getUserMessages()
    expect(userMessages.length).toBeGreaterThan(0)
    expect(userMessages[0]).toContain(firstQuestion)

    // Wait for assistant response to complete (free-tier LLM can be slow)
    await chatPage.waitForAssistantResponse(60_000)

    // Assert: at least one assistant message is visible
    const assistantMessages = await chatPage.getAssistantMessages()
    expect(assistantMessages.length).toBeGreaterThan(0)

    // Assert: suggested questions are gone
    await chatPage.waitForNoSuggestions()
  })

  // ---------------------------------------------------------------------------
  // Scenario 5: Not configured shows no suggestion buttons
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 5 - not configured shows no suggestion buttons', async ({
    page,
  }) => {
    // Intercept the suggestions API and return empty questions
    await page.route('**/api/chat/suggestions**', (route) => {
      route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ questions: [] }),
      })
    })

    await chatPage.navigate()
    await chatPage.waitForReady()

    // Assert: suggestions container is NOT visible (SuggestedQuestions returns null for empty array)
    await chatPage.waitForNoSuggestions()

    // Assert: chat still works normally — panel, input, send button all visible
    await expect(chatPage.panel).toBeVisible()
    await expect(chatPage.input).toBeVisible()
    await expect(chatPage.sendButton).toBeVisible()
  })

  // ---------------------------------------------------------------------------
  // Scenario 6: Manual input makes suggested question buttons disappear
  // ---------------------------------------------------------------------------
  test('US-CORE-028 scenario 6 - manual input makes suggested question buttons disappear', async () => {
    await chatPage.navigate()
    await chatPage.waitForReady()
    await chatPage.waitForSuggestedQuestions()

    // Assert: suggestions are visible before manual input
    await expect(chatPage.suggestedQuestionsContainer).toBeVisible()

    // Send a message via manual input (NOT clicking a suggested question)
    await chatPage.sendMessage('What is this system about?')

    // Wait for assistant response (free-tier LLM can be slow)
    await chatPage.waitForAssistantResponse(60_000)

    // Assert: suggested questions are gone after conversation starts via manual input
    await chatPage.waitForNoSuggestions()
  })
})
