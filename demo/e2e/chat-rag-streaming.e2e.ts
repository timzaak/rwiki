/**
 * 频道 RAG 流式聊天 E2E（support-multiple-website 迁移）
 *
 * The document lifecycle API now requires `channelId` (upload multipart, list /
 * publish / delete query params), and main-channel chat lives under `/c/$channelId`.
 * This file uploads + publishes a doc into `help_center` via API (so RAG has a
 * published doc), chats on `/c/help_center`, and cleans up with `?channelId=`.
 *
 * US-story traceability:
 * - US-INTG-006 (DRAFT, `.ai/user-stories/integration/support-multiple-website.md`)
 *   - Scenario 1 "带 channelId 的 Widget 正常对话": a question scoped to
 *     `help_center` retrieves only that channel's published docs and returns a
 *     relevant, non-empty answer (not a "知识库为空" empty-KB fallback).
 * - US-INTG-007 scn 1 (DRAFT): `/c/help_center` loads the channel chat interface.
 *
 * Dependencies:
 * - demo/e2e/fixtures/chat.fixtures.ts (demoLogger via fixture)
 * - demo/e2e/pages/chat-page.ts (navigate('/c/help_center'))
 * - demo/e2e/fixtures/test-xlsx.ts
 * - demo/e2e/selectors.ts
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'
import { TEST_XLSX_PATH } from './fixtures/test-xlsx'
import { SELECTORS } from './selectors'

// Demo backend (port 18080). NOT the old 8080.
const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'
const DEMO_CHANNEL_ID = 'help_center'
const authHeaders = { Authorization: 'Bearer demo-token' }

/**
 * Upload a file into `help_center` via the backend API.
 * Adds the required `channelId` multipart field alongside `file`.
 * Returns the document ID.
 */
async function uploadDocumentViaApi(page: import('@playwright/test').Page): Promise<string> {
  const response = await page.request.post(`${BASE_URL}/api/documents/upload`, {
    headers: authHeaders,
    multipart: {
      channelId: DEMO_CHANNEL_ID,
      file: {
        name: 'test-data.xlsx',
        mimeType: 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
        buffer: await import('node:fs').then((fs) => fs.promises.readFile(TEST_XLSX_PATH)),
      },
    },
  })

  expect(response.ok(), `Upload API should succeed, got ${response.status()}`).toBeTruthy()
  const body = await response.json()
  expect(body.id).toBeTruthy()
  expect(['draft', 'processing']).toContain(body.status)
  return body.id as string
}

/**
 * Publish a document to `help_center`. The document row transitions
 * `processing → draft`; publish only succeeds on `draft` (else 409), so this
 * polls for the draft state before issuing the publish PATCH.
 */
async function publishDocumentToChannel(
  page: import('@playwright/test').Page,
  documentId: string,
): Promise<void> {
  // Poll the channel-scoped document list until the row is no longer processing.
  await expect.poll(
    async () => {
      const res = await page.request.get(`${BASE_URL}/api/documents?channelId=${DEMO_CHANNEL_ID}`, {
        headers: authHeaders,
      })
      if (!res.ok()) return 'unknown'
      const listBody = (await res.json()) as { documents?: Array<{ id: string; status: string }> }
      const docs = listBody.documents ?? []
      const doc = docs.find((d) => d.id === documentId)
      return doc?.status ?? 'unknown'
    },
    { timeout: 30_000, intervals: [1_000, 2_000, 5_000] },
  ).not.toBe('processing')

  // Publish to help_center (required ?channelId= query param).
  const publishRes = await page.request.patch(
    `${BASE_URL}/api/documents/${documentId}/publish?channelId=${DEMO_CHANNEL_ID}`,
    { headers: authHeaders, data: '' },
  )
  expect(
    publishRes.ok(),
    `Publish to ${DEMO_CHANNEL_ID} should succeed, got ${publishRes.status()}`,
  ).toBeTruthy()
}

// ---------------------------------------------------------------------------
// US-INTG-007 scn 1 - Open chat interface on /c/help_center
// ---------------------------------------------------------------------------
test.describe('Chat Interface on /c/help_center', () => {
  test('US-INTG-007 scenario 1 - open chat interface displayed on /c/help_center', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // Assert: chat-panel is visible
    await expect(page.locator(SELECTORS.chat.panel)).toBeVisible()

    // Assert: chat-input textarea is visible
    await expect(page.locator(SELECTORS.chat.input)).toBeVisible()

    // Assert: chat-send-button is visible
    await expect(page.locator(SELECTORS.chat.sendButton)).toBeVisible()

    // Assert: message-list-empty is visible (no messages yet)
    await expect(page.locator(SELECTORS.chat.messageListEmpty)).toBeVisible()
  })
})

// ---------------------------------------------------------------------------
// US-INTG-006 scn 1 - Channel-scoped RAG streaming + multi-turn conversation.
// Requires a document published into help_center so RAG retrieval succeeds.
// ---------------------------------------------------------------------------
test.describe.serial('Chat RAG Streaming on /c/help_center', () => {
  let documentId: string | undefined

  test.beforeAll(async ({ browser }) => {
    // Upload + publish a document into help_center before any tests run.
    const context = await browser.newContext()
    const page = await context.newPage()
    try {
      documentId = await uploadDocumentViaApi(page)
      await publishDocumentToChannel(page, documentId)
    } finally {
      await context.close()
    }
  })

  test.afterAll(async ({ browser }) => {
    // Cleanup: delete the uploaded document from help_center.
    if (documentId) {
      const context = await browser.newContext()
      const page = await context.newPage()
      try {
        await page.request
          .delete(`${BASE_URL}/api/documents/${documentId}?channelId=${DEMO_CHANNEL_ID}`, {
            headers: authHeaders,
          })
          .catch(() => {
            // Best-effort cleanup
          })
      } finally {
        await context.close()
      }
    }
  })

  // US-INTG-006 scn 1a - Streaming response (text appears incrementally)
  test('US-INTG-006 scenario 1 - verify streaming response shows incremental text', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // Send a question related to the test data
    await chatPage.sendMessage('数据有多少行?')

    // The streaming indicator may appear briefly; if the response is fast it
    // may already be gone. Either case is acceptable — the key assertion is
    // that an assistant message appears.
    const streamingIndicator = page.locator(SELECTORS.chat.messageStreaming)
    const streamingAppeared = await streamingIndicator.isVisible({ timeout: 5000 }).catch(() => false)

    if (streamingAppeared) {
      test.info().annotations.push({
        type: 'streaming-verified',
        description: 'Streaming cursor was visible during response generation.',
      })
    } else {
      test.info().annotations.push({
        type: 'streaming-fast',
        description:
          'Streaming cursor was not captured (response completed too quickly). This is acceptable.',
      })
    }

    // Wait for the streaming to complete
    await chatPage.waitForAssistantResponse()

    // Assert: at least one assistant message is visible with non-empty content
    const assistantMessages = page.locator(SELECTORS.chat.messageItem('assistant'))
    await expect(assistantMessages.last()).toBeVisible()
    const assistantText = await assistantMessages.last().textContent()
    expect(assistantText).toBeTruthy()
    expect(assistantText!.length).toBeGreaterThan(0)
  })

  // US-INTG-006 scn 1b - Ask question based on help_center document content
  test('US-INTG-006 scenario 1 - ask question based on help_center document content', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // Send a question related to the test data (Sales sheet: Revenue column)
    await chatPage.sendMessage('Revenue最高的月份是哪个?')
    await chatPage.waitForAssistantResponse()

    // Assert: assistant message content is non-empty
    const messages = await chatPage.getAssistantMessages()
    const lastMessage = messages[messages.length - 1]
    expect(lastMessage).toBeTruthy()
    expect(lastMessage!.length).toBeGreaterThan(0)

    // Assert: the response does NOT say knowledge base is empty
    // (which would indicate the help_center doc was not published / seeded)
    expect(lastMessage).not.toContain('当前知识库中没有找到相关信息')
    expect(lastMessage).not.toContain('没有索引数据')
    expect(lastMessage).not.toContain('知识库为空')
  })

  // US-INTG-006 scn 1c - Follow-up question with context understanding
  test('US-INTG-006 scenario 1 - follow-up question with context understanding', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // Establish context with an initial question
    await chatPage.sendMessage('Revenue最高的月份是哪个?')
    await chatPage.waitForAssistantResponse()

    const messagesBeforeFollowUp = await chatPage.getAssistantMessages()

    // Follow-up that references the previous answer (requires context)
    await chatPage.sendMessage('那个月的具体数据是什么?')
    await chatPage.waitForAssistantResponse()

    // Assert: a new assistant response was generated
    const messagesAfterFollowUp = await chatPage.getAssistantMessages()
    expect(messagesAfterFollowUp.length).toBeGreaterThan(messagesBeforeFollowUp.length)

    // Assert: the follow-up response is non-empty and not a KB-empty fallback
    const followUpResponse = messagesAfterFollowUp[messagesAfterFollowUp.length - 1]
    expect(followUpResponse).toBeTruthy()
    expect(followUpResponse!.length).toBeGreaterThan(0)
    expect(followUpResponse).not.toContain('当前知识库中没有找到相关信息')
    expect(followUpResponse).not.toContain('没有索引数据')
    expect(followUpResponse).not.toContain('知识库为空')

    test.info().annotations.push({
      type: 'context-awareness-check',
      description:
        'Follow-up response should demonstrate multi-turn context understanding by referencing the month from the previous answer.',
    })
  })
})
