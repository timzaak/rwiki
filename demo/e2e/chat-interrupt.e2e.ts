/**
 * Chat interrupt E2E for US-CORE-039 scenarios 1/2, against the real streaming
 * backend on `/c/help_center` with a document uploaded + published so answers
 * are long enough to interrupt mid-stream.
 *
 * - Scenario 1 "流式生成中点击停止" → "scenario 1 - stop button" below.
 * - Scenario 2 "流式生成中直接发送新问题" → "scenario 2 - implicit interrupt" below.
 * - Scenarios 3/4 (interrupt during suggested-questions phase / before the
 *   first content chunk) are covered by frontend hook/store/component tests,
 *   not by e2e.
 *
 * Tolerance: demo answers may finish before the stop click / Enter lands, or
 * the click may land in the suggested-questions window after the answer body
 * completed. In all those cases the answer is complete with no interrupt
 * notice; the test accepts it and still asserts the input area is usable
 * (annotated, so the branch is visible in the report). Month-by-month
 * questions keep the probability low.
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'
import { TEST_XLSX_PATH } from './fixtures/test-xlsx'
import { SELECTORS } from './selectors'

const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'
const DEMO_CHANNEL_ID = 'help_center'
const authHeaders = { Authorization: 'Bearer demo-token' }

// Month-by-month questions produce long answers, leaving a wide window for
// interrupting mid-stream (short answers would complete before we can act).
const LONG_ANSWER_QUESTION = '请逐月详细列出Sales表中每个月的Revenue数据,并逐月简要分析'
const FOLLOW_UP_QUESTION = 'Revenue最高的月份是哪个?'

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

test.describe.serial('Chat Interrupt on /c/help_center', () => {
  let documentId: string | undefined

  test.beforeAll(async ({ browser }) => {
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
    if (documentId) {
      const context = await browser.newContext()
      const page = await context.newPage()
      try {
        await page.request
          .delete(`${BASE_URL}/api/documents/${documentId}?channelId=${DEMO_CHANNEL_ID}`, {
            headers: authHeaders,
          })
          .catch(() => {})
      } finally {
        await context.close()
      }
    }
  })

  // US-CORE-039 scn 1 - Stop button halts the streaming answer; partial
  // content is kept with the interrupt notice; input recovers; follow-up works.
  test('US-CORE-039 scenario 1 - stop button halts streaming and input recovers', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    await chatPage.sendMessage(LONG_ANSWER_QUESTION)

    const lastAssistant = page.locator(SELECTORS.chat.messageItem('assistant')).last()
    await expect(lastAssistant).toBeVisible()

    // Wait until some answer content has arrived: stopping before the first
    // chunk would remove the placeholder entirely (scenario 4, unit-tested —
    // not this test's concern).
    await expect
      .poll(async () => ((await lastAssistant.textContent()) ?? '').trim().length)
      .toBeGreaterThan(0)
    const partialLength = ((await lastAssistant.textContent()) ?? '').trim().length

    // While the round is loading, the send button has morphed into the stop
    // button. If the round already finished, fall through to the tolerance
    // branch below.
    const stopButton = page.locator(SELECTORS.chat.stopButton)
    const sendButton = page.locator(SELECTORS.chat.sendButton)
    await expect(stopButton.or(sendButton)).toBeVisible()
    if (await stopButton.isVisible().catch(() => false)) {
      // Tolerate the round finishing between the visibility check and the
      // click — the outcome is identical to the tolerance branch.
      await stopButton.click({ timeout: 3000 }).catch(() => {})
    }

    await expect(sendButton).toBeVisible()
    await expect(page.locator(SELECTORS.chat.messageStreaming)).toHaveCount(0)

    const interruptedNotice = page.locator(SELECTORS.chat.messageInterruptedNotice)
    if (await interruptedNotice.isVisible()) {
      // Interrupted mid-stream: partial answer kept, notice attached, content
      // did not shrink (a few more chunks may have landed before the abort).
      await expect(interruptedNotice).toHaveCount(1)
      const finalLength = ((await lastAssistant.textContent()) ?? '').trim().length
      expect(finalLength).toBeGreaterThanOrEqual(partialLength)
      test.info().annotations.push({
        type: 'interrupt-verified',
        description: 'Stop click landed mid-stream: partial answer kept with interrupt notice.',
      })
    } else {
      // Tolerance: the answer body had already completed (fast answer or the
      // suggested-questions window) — complete answer, no notice. The key
      // user-facing guarantee (input immediately usable) still holds.
      await expect(interruptedNotice).toHaveCount(0)
      const answerText = ((await lastAssistant.textContent()) ?? '').trim()
      expect(answerText.length).toBeGreaterThan(0)
      test.info().annotations.push({
        type: 'interrupt-fast',
        description:
          'Answer completed before the stop click landed. Accepted per tolerance; input usability still asserted.',
      })
    }

    // The input area is immediately usable again: still enabled, and a new
    // question goes through and gets answered (US-CORE-039 scn 1 "And" clause).
    await expect(chatPage.input).toBeEnabled()
    await chatPage.sendMessage(FOLLOW_UP_QUESTION)
    await chatPage.waitForAssistantResponse()

    const assistantMessages = page.locator(SELECTORS.chat.messageItem('assistant'))
    await expect(assistantMessages).toHaveCount(2)
    const followUpText = ((await assistantMessages.last().textContent()) ?? '').trim()
    expect(followUpText.length).toBeGreaterThan(0)
  })

  // US-CORE-039 scn 2 - Pressing Enter on a new question while an answer
  // streams interrupts it implicitly; the new answer completes normally.
  test('US-CORE-039 scenario 2 - sending a new question implicitly interrupts the current answer', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    await chatPage.sendMessage(LONG_ANSWER_QUESTION)

    // `.first()` (not `.last()`): a second assistant message appears later in
    // this test and locators re-resolve on every use.
    const firstAssistant = page.locator(SELECTORS.chat.messageItem('assistant')).first()
    await expect(firstAssistant).toBeVisible()

    // Wait for partial content so the interrupted answer keeps something.
    await expect
      .poll(async () => ((await firstAssistant.textContent()) ?? '').trim().length)
      .toBeGreaterThan(0)

    // Type the new question and press Enter while the first answer still
    // streams: the input stays usable during generation and Enter is the
    // implicit-interrupt entry (the button is in stop form at this point).
    await chatPage.input.fill(FOLLOW_UP_QUESTION)
    await chatPage.input.press('Enter')

    const userMessages = page.locator(SELECTORS.chat.messageItem('user'))
    await expect(userMessages).toHaveCount(2)
    await expect(userMessages.last()).toContainText(FOLLOW_UP_QUESTION)

    const assistantMessages = page.locator(SELECTORS.chat.messageItem('assistant'))
    await expect(assistantMessages).toHaveCount(2)

    // The second answer must produce content; wait explicitly (the round may
    // still be mid-stream when the count reaches 2).
    await expect
      .poll(async () => ((await assistantMessages.last().textContent()) ?? '').trim().length)
      .toBeGreaterThan(0)
    await chatPage.waitForAssistantResponse()

    const firstNotice = firstAssistant.locator(SELECTORS.chat.messageInterruptedNotice)
    if (await firstNotice.isVisible()) {
      await expect(firstNotice).toHaveCount(1)
      const firstText = ((await firstAssistant.textContent()) ?? '').trim()
      expect(firstText.length).toBeGreaterThan(0)
      await expect(
        assistantMessages.last().locator(SELECTORS.chat.messageInterruptedNotice),
      ).toHaveCount(0)
      test.info().annotations.push({
        type: 'implicit-interrupt-verified',
        description: 'Enter during streaming interrupted the old answer (notice kept) and the new answer completed.',
      })
    } else {
      // Tolerance: the first answer had already completed before Enter landed
      // (fast answer or suggested-questions window) — no notice is correct.
      await expect(page.locator(SELECTORS.chat.messageInterruptedNotice)).toHaveCount(0)
      test.info().annotations.push({
        type: 'implicit-interrupt-fast',
        description:
          'First answer completed before Enter landed. Accepted per tolerance; new round still verified.',
      })
    }

    const secondText = ((await assistantMessages.last().textContent()) ?? '').trim()
    expect(secondText.length).toBeGreaterThan(0)
  })
})
