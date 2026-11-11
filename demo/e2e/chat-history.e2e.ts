/**
 * Chat history E2E for US-CORE-048 / US-CORE-049 / US-CORE-051 (browser-local
 * conversation archive), against the real frontend on `/c/$channelId`.
 *
 * - US-CORE-048 (list + round-trip switching): path 1 below — empty state on
 *   first use, new-conversation auto-archives, A↔B round trips with the
 *   current entry highlighted (aria-current), clicking the current entry
 *   only collapses the list.
 * - US-CORE-049 (permanent local retention + channel isolation): path 1
 *   reload assertion (no 30-min TTL anymore) and path 2 (history of
 *   help_center is invisible on developer_docs, and still intact when
 *   switching back — isolation must not lose data).
 * - US-CORE-051 (delete / clear-all): path 3 — single delete executes
 *   immediately with no confirm dialog; clear-all opens the confirm dialog
 *   whose keyboard path is pinned (initial focus on cancel, Tab cycles
 *   cancel↔submit in both directions, Escape cancels); confirming clears
 *   the archived history while the current conversation stays intact.
 *
 * - US-CORE-050 (continue an archived conversation / transparent memory
 *   rebuild) is intentionally NOT covered here: multi-day-old archives and
 *   server-side session expiry cannot be constructed in the demo
 *   environment; the frontend guarantees (sessionId restore, no new error
 *   UI) are pinned by store/component tests.
 *
 * History lives in localStorage (per-test browser context = clean slate).
 * Answers stream from the real demo LLM; we only assert non-empty content
 * and full question text preservation, never answer wording.
 */

import { test, expect } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'
import { SELECTORS } from './selectors'

const DEMO_CHANNEL_ID = 'help_center'
// A second configured channel (no KB, no suggested questions) used only to
// observe the isolation boundary — nothing is ever asked there.
const OTHER_CHANNEL_ID = 'developer_docs'

// Keep questions under ~40 chars (the history title truncation limit) and
// mutually non-prefix so list-entry `hasText` filters match exactly one item.
const Q_A = 'What is this knowledge base about?'
const Q_B = 'What file formats are supported'
const Q3a = 'Which file formats can I upload?'
const Q3b = 'How do I search the knowledge base?'
const Q3c = 'What topics do the tutorials cover?'

/** The history list entry whose title contains `title`. */
function historyItemByTitle(chatPage: ChatPage, title: string) {
  return chatPage.modal
    .locator(SELECTORS.chat.historyItem)
    .filter({ hasText: title })
}

/** Open the history list overlay via the toolbar toggle. */
async function openHistoryList(chatPage: ChatPage): Promise<void> {
  const page = chatPage.page
  await page.locator(SELECTORS.chat.historyToggleButton).click()
  await expect(page.locator(SELECTORS.chat.historyPanel)).toBeVisible()
}

/** Collapse the history list with Escape. */
async function closeHistoryListByEscape(chatPage: ChatPage): Promise<void> {
  const page = chatPage.page
  await page.keyboard.press('Escape')
  await expect(page.locator(SELECTORS.chat.historyPanel)).toHaveCount(0)
}

/**
 * Click "new conversation": archives the current conversation into local
 * history and empties the workspace (message list back to its empty state).
 */
async function startNewConversation(chatPage: ChatPage): Promise<void> {
  const page = chatPage.page
  await page.locator(SELECTORS.chat.newConversationButton).click()
  await expect(page.locator(SELECTORS.chat.messageListEmpty)).toBeVisible()
}

test.describe('Chat history on /c/help_center (US-CORE-048/049/051)', () => {
  // Path 1 — US-CORE-048 scenarios 1/2/4 + US-CORE-049 scenario 1: the
  // archive-and-switch loop and reload persistence.
  test('US-CORE-048/049 - list, switch back and forth, keep history across reload', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // US-CORE-048 scn 4: first use → the list shows its empty state.
    await openHistoryList(chatPage)
    await expect(page.locator(SELECTORS.chat.historyEmpty)).toBeVisible()
    await closeHistoryListByEscape(chatPage)

    // Conversation A, then archive it via "new conversation".
    await chatPage.sendMessage(Q_A)
    await chatPage.waitForAssistantResponse(60_000)
    await startNewConversation(chatPage)

    // Conversation B, archived the same way.
    await chatPage.sendMessage(Q_B)
    await chatPage.waitForAssistantResponse(60_000)
    await startNewConversation(chatPage)

    // US-CORE-048 scn 2: both conversations were auto-archived (no manual
    // save); with an empty workspace the list holds exactly the two entries.
    await openHistoryList(chatPage)
    await expect(page.locator(SELECTORS.chat.historyItem)).toHaveCount(2)
    await expect(historyItemByTitle(chatPage, Q_A)).toBeVisible()
    await expect(historyItemByTitle(chatPage, Q_B)).toBeVisible()

    // US-CORE-048 scn 1: A → B → A round trips keep full content.
    await historyItemByTitle(chatPage, Q_A)
      .locator(SELECTORS.chat.historyItemTitle)
      .click()
    await expect(page.locator(SELECTORS.chat.historyPanel)).toHaveCount(0)
    expect(await chatPage.getUserMessages()).toEqual([Q_A])
    expect(((await chatPage.getAssistantMessages())[0] ?? '').trim().length).toBeGreaterThan(0)

    await openHistoryList(chatPage)
    // The viewed conversation is the highlighted current entry.
    await expect(page.locator(SELECTORS.chat.historyItemCurrent)).toContainText(Q_A)
    await historyItemByTitle(chatPage, Q_B)
      .locator(SELECTORS.chat.historyItemTitle)
      .click()
    expect(await chatPage.getUserMessages()).toEqual([Q_B])

    await openHistoryList(chatPage)
    await expect(page.locator(SELECTORS.chat.historyItemCurrent)).toContainText(Q_B)
    await historyItemByTitle(chatPage, Q_A)
      .locator(SELECTORS.chat.historyItemTitle)
      .click()
    expect(await chatPage.getUserMessages()).toEqual([Q_A])

    // Clicking the current entry only collapses the list (stays on A).
    await openHistoryList(chatPage)
    await page.locator(SELECTORS.chat.historyItemCurrent).click()
    await expect(page.locator(SELECTORS.chat.historyPanel)).toHaveCount(0)
    expect(await chatPage.getUserMessages()).toEqual([Q_A])

    // US-CORE-049 scn 1: reload — the workspace restores the most recent
    // conversation (A) and the archived one (B) is still in the list, with
    // its content fully viewable. This is the browser-level pin for the
    // removed 30-minute TTL.
    await page.reload()
    await chatPage.navigate(DEMO_CHANNEL_ID)
    expect(await chatPage.getUserMessages()).toEqual([Q_A])
    await openHistoryList(chatPage)
    await expect(page.locator(SELECTORS.chat.historyItemCurrent)).toContainText(Q_A)
    await expect(historyItemByTitle(chatPage, Q_B)).toBeVisible()
    await historyItemByTitle(chatPage, Q_B)
      .locator(SELECTORS.chat.historyItemTitle)
      .click()
    expect(await chatPage.getUserMessages()).toEqual([Q_B])
    expect(((await chatPage.getAssistantMessages())[0] ?? '').trim().length).toBeGreaterThan(0)
  })

  // Path 2 — US-CORE-049 scenario 2: history is isolated per channel. The
  // channel switch itself auto-archives the ongoing conversation into its
  // own channel (the /c/$channelId binding), so the data must survive while
  // becoming invisible on the other channel.
  test('US-CORE-049 scn 2 - history stays isolated between channels', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // One conversation on help_center (left un-archived: the channel switch
    // below must archive it by itself).
    await chatPage.sendMessage(Q_A)
    await chatPage.waitForAssistantResponse(60_000)

    // Switch channels: the workspace conversation is archived under
    // help_center; developer_docs starts with a clean, isolated list.
    await chatPage.navigate(OTHER_CHANNEL_ID)
    await openHistoryList(chatPage)
    await expect(page.locator(SELECTORS.chat.historyEmpty)).toBeVisible()
    await expect(historyItemByTitle(chatPage, Q_A)).toHaveCount(0)
    await closeHistoryListByEscape(chatPage)

    // Isolation is not data loss: back on help_center the conversation is
    // still archived and fully viewable.
    await chatPage.navigate(DEMO_CHANNEL_ID)
    await openHistoryList(chatPage)
    const archivedItem = historyItemByTitle(chatPage, Q_A)
    await expect(archivedItem).toBeVisible()
    await archivedItem.locator(SELECTORS.chat.historyItemTitle).click()
    expect(await chatPage.getUserMessages()).toEqual([Q_A])
    expect(((await chatPage.getAssistantMessages())[0] ?? '').trim().length).toBeGreaterThan(0)
  })

  // Path 3 — US-CORE-051: single delete (no confirmation), clear-all with a
  // keyboard-guarded confirm dialog, and the current conversation surviving
  // the clear.
  test('US-CORE-051 - delete single, clear-all confirm/cancel, current conversation unaffected', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const chatPage = new ChatPage(page)
    await chatPage.navigate(DEMO_CHANNEL_ID)

    // Two archived conversations (a, b) plus an active one (c).
    await chatPage.sendMessage(Q3a)
    await chatPage.waitForAssistantResponse(60_000)
    await startNewConversation(chatPage)
    await chatPage.sendMessage(Q3b)
    await chatPage.waitForAssistantResponse(60_000)
    await startNewConversation(chatPage)
    await chatPage.sendMessage(Q3c)
    await chatPage.waitForAssistantResponse(60_000)

    await openHistoryList(chatPage)
    const currentEntry = page.locator(SELECTORS.chat.historyItemCurrent)
    await expect(currentEntry).toContainText(Q3c)
    // The current entry cannot be deleted (its destruction path is
    // new-conversation first, then delete the archive entry).
    await expect(
      currentEntry.locator(SELECTORS.chat.historyItemDeleteButton),
    ).toHaveCount(0)
    await expect(historyItemByTitle(chatPage, Q3a)).toBeVisible()
    await expect(historyItemByTitle(chatPage, Q3b)).toBeVisible()

    // US-CORE-051 scn 1: delete executes immediately — no confirm dialog,
    // only the target entry disappears.
    await historyItemByTitle(chatPage, Q3b)
      .locator(SELECTORS.chat.historyItemDeleteButton)
      .click()
    await expect(page.locator(SELECTORS.common.dialog)).toHaveCount(0)
    await expect(historyItemByTitle(chatPage, Q3b)).toHaveCount(0)
    await expect(historyItemByTitle(chatPage, Q3a)).toBeVisible()
    await expect(currentEntry).toContainText(Q3c)

    // US-CORE-051 scn 3 + keyboard guard: open clear-all confirm. Initial
    // focus sits on cancel (Enter cannot confirm by accident), Tab cycles
    // cancel↔submit in BOTH directions (the focus trap), Escape cancels and
    // leaves history untouched.
    await page.locator(SELECTORS.chat.historyClearAllButton).click()
    await expect(page.locator(SELECTORS.common.dialog)).toBeVisible()
    await expect(page.locator(SELECTORS.common.dialogTitle)).toBeVisible()
    await expect(page.locator(SELECTORS.common.dialogContent)).toBeVisible()

    await expect(
      page.locator(SELECTORS.common.dialogCancelButton),
    ).toBeFocused()
    await page.keyboard.press('Tab')
    await expect(
      page.locator(SELECTORS.common.dialogSubmitButton),
    ).toBeFocused()
    // Tab on the last button wraps back to the first one.
    await page.keyboard.press('Tab')
    await expect(
      page.locator(SELECTORS.common.dialogCancelButton),
    ).toBeFocused()
    // Shift+Tab on the first button wraps forward to the last one.
    await page.keyboard.press('Shift+Tab')
    await expect(
      page.locator(SELECTORS.common.dialogSubmitButton),
    ).toBeFocused()

    await page.keyboard.press('Escape')
    await expect(page.locator(SELECTORS.common.dialog)).toHaveCount(0)
    // Cancelled clear: the archived entry and the current entry remain.
    await expect(historyItemByTitle(chatPage, Q3a)).toBeVisible()
    await expect(historyItemByTitle(chatPage, Q3b)).toHaveCount(0)
    await expect(currentEntry).toContainText(Q3c)

    // US-CORE-051 scn 2: confirm clears the archived history; the current
    // conversation is untouched (still the only list entry, still fully
    // rendered in the chat area).
    await page.locator(SELECTORS.chat.historyClearAllButton).click()
    await expect(page.locator(SELECTORS.common.dialog)).toBeVisible()
    await page.locator(SELECTORS.common.dialogSubmitButton).click()
    await expect(page.locator(SELECTORS.common.dialog)).toHaveCount(0)
    await expect(historyItemByTitle(chatPage, Q3a)).toHaveCount(0)
    await expect(page.locator(SELECTORS.chat.historyItem)).toHaveCount(1)
    await expect(currentEntry).toContainText(Q3c)

    await closeHistoryListByEscape(chatPage)
    expect(await chatPage.getUserMessages()).toEqual([Q3c])
    expect(((await chatPage.getAssistantMessages())[0] ?? '').trim().length).toBeGreaterThan(0)
  })
})
