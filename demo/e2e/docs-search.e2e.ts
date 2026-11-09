/**
 * Docs site (docs-web) semantic search E2E — US-CORE-045.
 *
 * The docs Ctrl+K dialog calls the backend `POST /api/search` instead of the
 * former static client-side index:
 * - scn 1: typing sends nothing (Enter submits; request body carries the
 *   locale's channel — `help_center` on /en) and Enter on the shown results
 *   navigates to a same-site docs page.
 * - scn 2: the Chinese site searches `help_center_zh` and navigates to
 *   /zh/docs/... (seeded zh tutorials).
 * - scn 3: the "Ask AI" row (US-INTG-017) hands the query to the rwiki chat
 *   widget on the page: modal opens with the query as a user turn, and the
 *   chat request goes to the English channel.
 * - scn 4: switching site language switches the widget's channel
 *   (US-INTG-016): the next ask-AI chat request carries `help_center_zh`.
 * - scn 5: backend failure shows the fixed error copy (no raw errors, no
 *   result items).
 *
 * Retrieval includes an LLM query rewrite, so result counts/latency vary;
 * assertions are structural (request shape, navigation target), never exact
 * ranking. Requires `python scripts/demo-start.py` (or demo-test-runner.py)
 * which seeds both channels and serves the widget bundle with `ask`/`setChannel`.
 */

import type { Page, Request } from '@playwright/test'

import { test, expect, DOCS_URL } from './fixtures/chat.fixtures'
import { SELECTORS } from './selectors'

const SEARCH_TIMEOUT = 90_000 // retrieval includes an LLM rewrite + embedding

interface SearchBody {
  query?: string
  channelId?: string
  topK?: number
}

interface ChatBody {
  message?: string
  channelId?: string[]
}

/** 收集页面对后端 /api/search 与 /api/chat 的请求体（fire-and-forget 采样）。 */
function captureApiRequests(page: Page) {
  const searchBodies: SearchBody[] = []
  const chatBodies: ChatBody[] = []
  page.on('request', (request: Request) => {
    const url = request.url()
    if (request.method() !== 'POST') return
    if (url.endsWith('/api/search')) {
      searchBodies.push(JSON.parse(request.postData() || '{}'))
    } else if (url.endsWith('/api/chat')) {
      chatBodies.push(JSON.parse(request.postData() || '{}'))
    }
  })
  return { searchBodies, chatBodies }
}

async function openSearchDialog(page: Page) {
  // Hydration marker: the rwiki widget's floating button only mounts on the
  // client. Before hydration neither Ctrl+K nor a trigger click does anything
  // (SSR HTML is inert), which made the dialog "not open" flakily.
  await expect(page.locator(SELECTORS.chat.floatingButton)).toBeVisible({
    timeout: 30_000,
  })
  await page.keyboard.press('Control+k')
  const input = page.locator(SELECTORS.docsSearch.input)
  await expect(input).toBeVisible({ timeout: 15_000 })
  return input
}

/** 走完一次「输入 → Enter → 出结果」；返回结果条目 locator。 */
async function searchDocs(page: Page, query: string) {
  const input = await openSearchDialog(page)
  await input.fill(query)
  await input.press('Enter')
  const options = page.locator(SELECTORS.docsSearch.option)
  await expect(options.first()).toBeVisible({ timeout: SEARCH_TIMEOUT })
  return options
}

test.describe('Docs site semantic search (US-CORE-045)', () => {
  test('scn 1 - typing sends nothing; Enter searches help_center and navigates to a docs page', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const { searchBodies } = captureApiRequests(page)
    await page.goto(`${DOCS_URL}/en/docs`)

    const input = await openSearchDialog(page)
    await input.fill('getting started')
    // 仅输入不检索：主站同款成本规则在文档站同样成立。networkidle 的静默窗口
    // 内若出现「输入即搜」回归，请求会先落入 searchBodies 再断言失败。
    await page.waitForLoadState('networkidle')
    expect(searchBodies).toHaveLength(0)

    await input.press('Enter')
    await expect
      .poll(() => searchBodies.length, { timeout: SEARCH_TIMEOUT })
      .toBe(1)
    expect(searchBodies[0]).toEqual({
      query: 'getting started',
      channelId: 'help_center',
    })

    await expect(
      page.locator(SELECTORS.docsSearch.option).first(),
    ).toBeVisible({ timeout: SEARCH_TIMEOUT })
    // 有结果后回车 = 打开当前激活条目（第一条结果，非 AI 入口）。
    await input.press('Enter')
    await expect(page).toHaveURL(/\/en\/docs\/[a-z-]+/, { timeout: 30_000 })

    // 成本规则的兜底断言：场景结束时检索请求总数仍为 1。上面的 networkidle
    // 静默窗口抓不到长防抖的「输入即搜」回归，但那种请求会在 fill 与场景结束
    // 之间到达（检索耗时 + 导航等待远大于常见防抖窗口），使总数超过 1。
    await page.waitForLoadState('networkidle')
    expect(searchBodies).toHaveLength(1)
  })

  test('scn 2 - the Chinese site searches help_center_zh and navigates to /zh/docs', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const { searchBodies } = captureApiRequests(page)
    await page.goto(`${DOCS_URL}/zh/docs`)

    const input = await openSearchDialog(page)
    await input.fill('如何快速上手')
    await input.press('Enter')

    await expect
      .poll(() => searchBodies.length, { timeout: SEARCH_TIMEOUT })
      .toBe(1)
    expect(searchBodies[0]).toMatchObject({
      query: '如何快速上手',
      channelId: 'help_center_zh',
    })

    await expect(
      page.locator(SELECTORS.docsSearch.option).first(),
    ).toBeVisible({ timeout: SEARCH_TIMEOUT })
    await input.press('Enter')
    await expect(page).toHaveURL(/\/zh\/docs\/[a-z-]+/, { timeout: 30_000 })
  })

  test('scn 3 - Ask AI hands the query to the chat widget on the English channel', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const { chatBodies } = captureApiRequests(page)
    await page.goto(`${DOCS_URL}/en/docs`)

    const options = await searchDocs(page, 'getting started')
    await expect(options.first()).toBeVisible()

    const askAi = page.locator(SELECTORS.docsSearch.askAi)
    await expect(askAi).toBeVisible()
    await expect(askAi).toContainText('getting started')

    await askAi.click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible({
      timeout: 15_000,
    })
    await expect(
      page.locator(SELECTORS.chat.messageItem('user')).first(),
    ).toContainText('getting started')

    await expect
      .poll(() => chatBodies.length, { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1)
    expect(chatBodies[0]).toMatchObject({
      message: 'getting started',
      channelId: ['help_center'],
    })
  })

  test('scn 4 - switching site language switches the widget channel', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const { chatBodies } = captureApiRequests(page)
    await page.goto(`${DOCS_URL}/en/docs`)

    // 先在英文站发起一次 AI 问答（widget 落在 help_center）。
    const options = await searchDocs(page, 'getting started')
    await expect(options.first()).toBeVisible()
    await page.locator(SELECTORS.docsSearch.askAi).click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible({
      timeout: 15_000,
    })
    // Close via the modal's close button (the widget modal has no Esc binding,
    // and its overlay would swallow the language-switcher click).
    await page.locator(SELECTORS.chat.modalClose).click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeHidden({ timeout: 10_000 })

    // 语言切换（客户端路由，不整页刷新，widget 不重建）。
    await page.getByRole('button', { name: 'Choose a language' }).click()
    await page.getByRole('button', { name: '中文', exact: true }).click()
    await expect(page).toHaveURL(/\/zh\/docs/, { timeout: 15_000 })

    // 中文站再次「用 AI 回答」：请求必须发给中文频道（setChannel 生效）。
    const zhOptions = await searchDocs(page, '如何快速上手')
    await expect(zhOptions.first()).toBeVisible()
    await page.locator(SELECTORS.docsSearch.askAi).click()
    await expect(page.locator(SELECTORS.chat.modal)).toBeVisible({
      timeout: 15_000,
    })

    const chatRequests = chatBodies.filter((b) => b.message === '如何快速上手')
    await expect
      .poll(() => chatRequests.length, { timeout: 30_000 })
      .toBeGreaterThanOrEqual(1)
    expect(chatRequests[0]).toMatchObject({
      message: '如何快速上手',
      channelId: ['help_center_zh'],
    })
  })

  test('scn 5 - backend failure shows fixed error copy and no results', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    await page.route('**/api/search', (route) =>
      route.fulfill({ status: 500, json: { code: 500, message: 'boom' } }),
    )
    await page.goto(`${DOCS_URL}/en/docs`)

    const input = await openSearchDialog(page)
    await input.fill('getting started')
    await input.press('Enter')

    const status = page.locator(SELECTORS.docsSearch.status)
    await expect(status).toBeVisible({ timeout: 15_000 })
    await expect(status).toHaveText('Search failed. Please try again later.')
    await expect(page.locator(SELECTORS.docsSearch.option)).toHaveCount(0)
    await expect(page.locator(SELECTORS.docsSearch.askAi)).toHaveCount(0)
  })
})
