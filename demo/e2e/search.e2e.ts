/**
 * Dedicated search E2E for US-CORE-041 scenarios 1-3 on the main-site channel
 * page (`/c/$channelId` ready branch, where the search box renders).
 *
 * - Scenario 1 "搜索成功" → "scenario 1" below: typing must send nothing (only
 *   Enter / the button may trigger a search), one request per submission,
 *   results carry title + summary, and clicking a linked result opens its
 *   source page in a new tab. The source domain is answered via `context.route`
 *   so the demo never depends on the external site.
 * - Scenario 2 "无相关内容（零命中）" → "scenario 2": `developer_docs` is
 *   configured but seeded with no published docs — the only stable zero-hit
 *   channel (retrieval has no relevance threshold, so a content-bearing channel
 *   returns low-relevance results instead of an empty list).
 * - Scenario 3 "检索进行中" → "scenario 3": the /api/search route is only
 *   delayed (never fulfilled) so the loading state is observable, then released
 *   and replaced by real results.
 *
 * Scenario 4 (result without a source link) and the ranking story US-CORE-042
 * are covered by frontend Vitest and backend tests respectively; this file
 * asserts structure, not ranking.
 *
 * US-CORE-043 (highlight + keyboard) and US-CORE-044 (ask AI) run against the
 * same seeded `help_center` channel. Retrieval includes a non-deterministic LLM
 * rewrite, so the highlight assertion is derived from the rendered results
 * rather than from a fixed expected hit.
 * IME guards, query/draft coupling and zero-hit hiding of the AI entry are
 * component rules covered by frontend Vitest.
 */

import type { Locator, Page } from '@playwright/test'

import { test, expect, FRONTEND_URL } from './fixtures/chat.fixtures'
import { ChatPage } from './pages/chat-page'
import { SELECTORS } from './selectors'

// Seeded by scripts/demo-start.py with tutorials whose frontmatter carries a
// source link, so retrieval returns linked results for the tutorial title.
const HELP_CENTER = 'help_center'
// Configured in the demo backend but left without published docs (same premise
// as the widget empty-channel tests) — the stable zero-hit channel.
const EMPTY_CHANNEL = 'developer_docs'
const QUERY = 'getting started'

const SEARCH_API_PATTERN = /\/api\/search(?:\?|$)/

// 把结果来源域拦到本地应答（demo 不依赖外部站点），触发 link 激活，断言
// 新标签页 URL 等于锚点 href。scenario 1（点击）与 US-CORE-043（键盘
// Enter）共用，trigger 只负责怎么激活链接。
async function expectOpensSourceInNewTab(
  page: Page,
  link: Locator,
  trigger: () => Promise<void>,
) {
  const href = await link.getAttribute('href')
  expect(href, 'seeded tutorial results must carry a source link').toBeTruthy()
  const context = page.context()
  const sourceOrigin = new URL(href as string).origin
  await context.route(`${sourceOrigin}/**`, (route) =>
    route.fulfill({ status: 200, contentType: 'text/html', body: 'demo source page' }),
  )
  const newPagePromise = context.waitForEvent('page')
  await trigger()
  const sourcePage = await newPagePromise
  await expect(sourcePage).toHaveURL(href as string)
  await context.unroute(`${sourceOrigin}/**`)
}

test.describe('Dedicated search on /c/$channelId', () => {
  // US-CORE-041 scn 1 - typing sends nothing; Enter submits once; results show
  // title + summary; a linked result opens its source in a new tab; the button
  // path submits too.
  test('US-CORE-041 scenario 1 - search submits on Enter and button, opens source in new tab', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    let searchRequests = 0
    page.on('request', (request) => {
      if (request.method() === 'POST' && SEARCH_API_PATTERN.test(request.url())) {
        searchRequests += 1
      }
    })

    await page.goto(`${FRONTEND_URL}/c/${HELP_CENTER}`)
    const searchInput = page.locator(SELECTORS.search.input)
    await expect(searchInput).toBeVisible()

    await searchInput.pressSequentially(QUERY, { delay: 25 })
    // Input alone must never fire a search request.
    expect(searchRequests).toBe(0)

    await searchInput.press('Enter')
    const firstItem = page.locator(SELECTORS.search.resultItem).first()
    // Retrieval is pure FTS (no rewrite/embedding), but the first e2e request
    // still pays backend startup costs, so keep a generous timeout.
    await expect(firstItem).toBeVisible({ timeout: 60_000 })
    expect(searchRequests).toBe(1)
    expect((await firstItem.textContent())?.trim()).not.toBe('')

    // Answer the source domain locally; the seeded link must not need the
    // external site to open as a new tab whose URL equals the anchor href.
    const firstLink = page.locator(SELECTORS.search.resultLink).first()
    await expectOpensSourceInNewTab(page, firstLink, () => firstLink.click())

    // The button path must submit as well (Enter was used above).
    await page.locator(SELECTORS.search.submitButton).click()
    await expect(firstItem).toBeVisible({ timeout: 60_000 })
    expect(searchRequests).toBe(2)
  })

  // US-CORE-041 scn 2 - a zero-hit channel (no published docs) shows the
  // no-results hint; it is not an error.
  test('US-CORE-041 scenario 2 - zero-hit channel shows no-results hint without error', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.goto(`${FRONTEND_URL}/c/${EMPTY_CHANNEL}`)
    const searchInput = page.locator(SELECTORS.search.input)
    await expect(searchInput).toBeVisible()

    await searchInput.fill(QUERY)
    await page.locator(SELECTORS.search.submitButton).click()

    await expect(page.locator(SELECTORS.search.empty)).toBeVisible({ timeout: 60_000 })
    await expect(page.locator(SELECTORS.search.error)).toHaveCount(0)
  })

  // US-CORE-041 scn 3 - while retrieval is in flight the loading state shows;
  // once results arrive it is replaced by the result list.
  test('US-CORE-041 scenario 3 - loading state appears and is replaced by results', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    let releaseRequest!: () => void
    const gate = new Promise<void>((resolve) => {
      releaseRequest = resolve
    })
    // Hold the request and only delay continue() — never fake the response,
    // so the final assertions run against real retrieval results.
    await page.route('**/api/search', async (route) => {
      await gate
      await route.continue()
    })

    await page.goto(`${FRONTEND_URL}/c/${HELP_CENTER}`)
    const searchInput = page.locator(SELECTORS.search.input)
    await expect(searchInput).toBeVisible()

    await searchInput.fill(QUERY)
    await searchInput.press('Enter')

    const loading = page.locator(SELECTORS.search.loading)
    await expect(loading).toBeVisible()
    releaseRequest()
    await expect(page.locator(SELECTORS.search.resultItem).first()).toBeVisible({ timeout: 60_000 })
    await expect(loading).toHaveCount(0)
  })

  // US-CORE-043 - query words are highlighted; ↓ walks from the input to the
  // AI entry, then to the first linked result, and Enter opens it in a new tab.
  test('US-CORE-043 - highlights query words and opens a result via the keyboard', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.goto(`${FRONTEND_URL}/c/${HELP_CENTER}`)
    const searchInput = page.locator(SELECTORS.search.input)
    await expect(searchInput).toBeVisible()

    await searchInput.fill(QUERY)
    await searchInput.press('Enter')
    await expect(page.locator(SELECTORS.search.resultItem).first()).toBeVisible({ timeout: 60_000 })

    // Derived from the real results: every mark is a query word, and every
    // query word appearing in a title/summary (the <p> lines; the link line is
    // never highlighted) is marked somewhere.
    const queryWords = QUERY.split(' ')
    const marks = (await page.locator(SELECTORS.search.highlight).allTextContents()).map(
      (text) => text.toLowerCase(),
    )
    for (const mark of marks) {
      expect(queryWords).toContain(mark)
    }
    const bodyText = (await page.locator(`${SELECTORS.search.resultItem} p`).allTextContents())
      .join(' ')
      .toLowerCase()
    for (const word of queryWords) {
      if (bodyText.includes(word)) expect(marks).toContain(word)
    }

    await searchInput.press('ArrowDown')
    await expect(page.locator(SELECTORS.search.askAi)).toBeFocused()
    await page.keyboard.press('ArrowDown')
    const firstLink = page.locator(SELECTORS.search.resultLink).first()
    await expect(firstLink).toBeFocused()

    // Enter on the focused link opens its source in a new tab (scenario 1's
    // local-answer contract, triggered from the keyboard).
    await expectOpensSourceInNewTab(page, firstLink, () => page.keyboard.press('Enter'))
  })

  // US-CORE-044 - "用 AI 回答" opens the chat modal with the searched query as
  // the user turn, and a real RAG answer streams back.
  test('US-CORE-044 - ask AI hands the searched query over to the chat modal', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.goto(`${FRONTEND_URL}/c/${HELP_CENTER}`)
    const searchInput = page.locator(SELECTORS.search.input)
    await expect(searchInput).toBeVisible()

    await searchInput.fill(QUERY)
    await searchInput.press('Enter')
    const askAi = page.locator(SELECTORS.search.askAi)
    await expect(askAi).toBeVisible({ timeout: 60_000 })
    await expect(askAi).toContainText(QUERY)

    await askAi.click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible()
    await expect(page.locator(SELECTORS.search.dropdown)).toHaveCount(0)
    await expect(page.locator(SELECTORS.chat.messageItem('user')).first()).toContainText(QUERY)

    // Wait for the answer to finish streaming, then require real content.
    await new ChatPage(page).waitForAssistantResponse(60_000)
    const assistant = page.locator(SELECTORS.chat.messageItem('assistant')).last()
    expect((await assistant.textContent())?.trim()).not.toBe('')

    // The keyword stays in the search box for refining.
    await expect(searchInput).toHaveValue(QUERY)
  })
})
