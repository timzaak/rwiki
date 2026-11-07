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
 */

import { test, expect, FRONTEND_URL } from './fixtures/chat.fixtures'
import { SELECTORS } from './selectors'

// Seeded by scripts/demo-start.py with tutorials whose frontmatter carries a
// source link, so retrieval returns linked results for the tutorial title.
const HELP_CENTER = 'help_center'
// Configured in the demo backend but left without published docs (same premise
// as the widget empty-channel tests) — the stable zero-hit channel.
const EMPTY_CHANNEL = 'developer_docs'
const QUERY = 'getting started'

const SEARCH_API_PATTERN = /\/api\/search(?:\?|$)/

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
    // Retrieval includes an LLM rewrite pass, so results can take a while.
    await expect(firstItem).toBeVisible({ timeout: 60_000 })
    expect(searchRequests).toBe(1)
    expect((await firstItem.textContent())?.trim()).not.toBe('')

    // Answer the source domain locally; the seeded link must not need the
    // external site to open as a new tab whose URL equals the anchor href.
    const firstLink = page.locator(SELECTORS.search.resultLink).first()
    const href = await firstLink.getAttribute('href')
    expect(href, 'seeded tutorial results must carry a source link').toBeTruthy()
    const context = page.context()
    const sourceOrigin = new URL(href as string).origin
    await context.route(`${sourceOrigin}/**`, (route) =>
      route.fulfill({ status: 200, contentType: 'text/html', body: 'demo source page' }),
    )
    const newPagePromise = context.waitForEvent('page')
    await firstLink.click()
    const sourcePage = await newPagePromise
    await expect(sourcePage).toHaveURL(href as string)
    await context.unroute(`${sourceOrigin}/**`)

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
})
