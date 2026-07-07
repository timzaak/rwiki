/**
 * 频道级推荐问题 E2E（support-multiple-website 迁移）
 *
 * The suggestions API now requires `channelId` (`GET /api/chat/suggestions?locale=&channelId=`)
 * and returns per-channel questions; a channel with no configured `suggested_questions`
 * returns an empty array (no fallback to global/widget). Main-channel chat lives under
 * `/c/$channelId`, so the chat-UI scenarios navigate to `/c/help_center` (or
 * `/c/developer_docs` for the empty-state case).
 *
 * US-story traceability:
 * - US-INTG-005/006/007 (DRAFT, `.ai/user-stories/integration/support-multiple-website.md`):
 *   per-channel suggestions are part of channel-scoped data isolation. The "configured"
 *   cases target `help_center` (which has channel-level `suggested_questions`); the
 *   "not configured" case targets `developer_docs` (intentionally empty) — a real
 *   API call, not a route intercept.
 *
 * Scenarios:
 * 1. help_center empty state shows suggestion buttons (configured)
 * 2. API locale matching returns correct language (zh-CN exact / en-US prefix → en)
 * 3. No locale match falls back to default
 * 4. Click suggested question sends message and buttons disappear
 * 5. developer_docs (not configured) shows no suggestion buttons (real empty API)
 * 6. Manual input makes suggested question buttons disappear
 *
 * Dependencies:
 * - demo/e2e/fixtures/chat.fixtures.ts (demoLogger via fixture)
 * - demo/e2e/pages/chat-page.ts (navigate('/c/$channelId'))
 * - demo/e2e/selectors.ts
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'

// Demo backend (port 18080). NOT the old 8080.
const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'
const HELP_CENTER = 'help_center'
const DEVELOPER_DOCS = 'developer_docs'

test.describe('Channel-level suggested questions', () => {
  let chatPage: ChatPage

  test.beforeEach(async ({ page, demoLogger }) => {
    chatPage = new ChatPage(page, demoLogger)
  })

  // ---------------------------------------------------------------------------
  // Scenario 1: help_center empty state shows suggestion buttons when configured
  // ---------------------------------------------------------------------------
  test('help_center empty state shows suggestion buttons when configured', async () => {
    await chatPage.navigate(HELP_CENTER)

    // Wait for suggestions to load from the channel-scoped API
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
  // Scenario 2: API locale matching (zh-CN exact / en-US prefix → en)
  // ---------------------------------------------------------------------------
  test('help_center API locale matching returns correct language questions', async ({
    page,
  }) => {
    // zh-CN returns the configured Chinese questions (exact match)
    const zhResponse = await page.request.get(
      `${BASE_URL}/api/chat/suggestions?locale=zh-CN&channelId=${HELP_CENTER}`,
    )
    expect(zhResponse.status()).toBe(200)
    const zhBody = await zhResponse.json()
    expect(zhBody.questions).toBeInstanceOf(Array)
    expect(zhBody.questions.length).toBeGreaterThan(0)
    // At least one question should contain Chinese characters
    const hasChinese = zhBody.questions.some((q: string) => /[一-鿿]/.test(q))
    expect(hasChinese).toBeTruthy()

    // en-US has no exact key, so it prefix-matches the "en" key → English questions
    const enResponse = await page.request.get(
      `${BASE_URL}/api/chat/suggestions?locale=en-US&channelId=${HELP_CENTER}`,
    )
    expect(enResponse.status()).toBe(200)
    const enBody = await enResponse.json()
    expect(enBody.questions).toBeInstanceOf(Array)
    expect(enBody.questions.length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 3: No locale match falls back to default
  // ---------------------------------------------------------------------------
  test('help_center no locale match falls back to default questions', async ({ page }) => {
    // French is not configured; falls back to the "default" group
    const response = await page.request.get(
      `${BASE_URL}/api/chat/suggestions?locale=fr&channelId=${HELP_CENTER}`,
    )
    expect(response.status()).toBe(200)
    const body = await response.json()
    expect(body.questions).toBeInstanceOf(Array)
    // The "default" group is configured, so questions should not be empty
    expect(body.questions.length).toBeGreaterThan(0)
  })

  // ---------------------------------------------------------------------------
  // Scenario 4: Click suggested question sends message and buttons disappear
  // ---------------------------------------------------------------------------
  test('help_center click suggested question sends message and buttons disappear', async () => {
    await chatPage.navigate(HELP_CENTER)
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
  // Scenario 5: developer_docs (not configured) shows no suggestion buttons
  // Uses the real API (developer_docs has no suggested_questions → empty array),
  // not a route intercept.
  // ---------------------------------------------------------------------------
  test('developer_docs not configured shows no suggestion buttons', async ({ page }) => {
    // Sanity: the developer_docs API genuinely returns an empty array.
    const response = await page.request.get(
      `${BASE_URL}/api/chat/suggestions?channelId=${DEVELOPER_DOCS}`,
    )
    expect(response.status()).toBe(200)
    const body = await response.json()
    expect(body.questions).toBeInstanceOf(Array)
    expect(body.questions).toHaveLength(0)

    await chatPage.navigate(DEVELOPER_DOCS)

    // Assert: suggestions container is NOT visible (empty array → component null)
    await chatPage.waitForNoSuggestions()

    // Assert: chat still works normally — panel, input, send button all visible
    await expect(chatPage.panel).toBeVisible()
    await expect(chatPage.input).toBeVisible()
    await expect(chatPage.sendButton).toBeVisible()
  })

  // ---------------------------------------------------------------------------
  // Scenario 6: Manual input makes suggested question buttons disappear
  // ---------------------------------------------------------------------------
  test('help_center manual input makes suggested question buttons disappear', async () => {
    await chatPage.navigate(HELP_CENTER)
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
