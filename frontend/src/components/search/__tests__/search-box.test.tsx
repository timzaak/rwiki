import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, within, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import type { SearchResultItem } from '@/lib/api-generated/types.gen'
import { SearchBox } from '@/components/search/search-box'

/**
 * US-CORE-041/042 — 主站搜索框交互契约（触发、状态、隔离）。
 *
 * WHY 这些用例存在（Demo 难以稳定覆盖）：
 *  - 输入过程零请求 + 精确请求体（不传 topK）是公开检索成本契约，
 *    Demo 只看计数，body 断言属契约级。
 *  - IME 组合回车、竞态、在途去重、清空放弃、收起/重开是组件内状态机，
 *    真实环境时序不可控。
 *  - 结果必须按接口返回顺序渲染（page 级排序下相邻 score 不单调），
 *    客户端按 score 重排会拆散同页片段。
 *  - 空结果只由 results: [] 驱动（零命中口径），不得当错误或被低分过滤。
 */

const BASE_URL = 'http://localhost:3000'
const SEARCH_URL = `${BASE_URL}/api/search`
const CHANNEL_ID = 'channel-a'

function makeItem(overrides: Partial<SearchResultItem> = {}): SearchResultItem {
  return {
    title: '教程页',
    content: '教程内容片段',
    link: 'https://docs.example.com/page',
    locale: 'zh',
    score: 0.5,
    section: null,
    documentId: 'doc-1',
    ...overrides,
  }
}

let searchCallCount: number
let capturedBodies: Array<Record<string, unknown>>
// 每个 in-flight 请求的 resolver，按请求顺序入队；测试侧决定放行顺序。
const pendingSearches: Array<(results: SearchResultItem[]) => void> = []

function installSearchHandler(
  respond: () => Response | Promise<Response> = () =>
    HttpResponse.json({ results: [makeItem()] }),
) {
  server.use(
    http.post(SEARCH_URL, async ({ request }) => {
      searchCallCount += 1
      capturedBodies.push((await request.json()) as Record<string, unknown>)
      return respond()
    }),
  )
}

function installDeferredSearch() {
  installSearchHandler(
    () =>
      new Promise<Response>((resolve) => {
        pendingSearches.push((results) =>
          resolve(HttpResponse.json({ results })),
        )
      }),
  )
}

async function submit(user: ReturnType<typeof userEvent.setup>, query: string) {
  await user.type(screen.getByTestId('search-input'), query)
  await user.keyboard('{Enter}')
}

beforeEach(() => {
  client.setConfig({ baseUrl: BASE_URL })
  searchCallCount = 0
  capturedBodies = []
  pendingSearches.length = 0
  vi.clearAllMocks()
})

describe('search submission trigger and request body', () => {
  it('fires zero requests while typing; Enter and button each submit once with trimmed query + channelId and no topK', async () => {
    const user = userEvent.setup()
    installSearchHandler()
    render(<SearchBox channelId={CHANNEL_ID} />)

    // 逐字输入期间零请求（回车/按钮触发，不做输入即时搜索）。
    await user.type(screen.getByTestId('search-input'), '  getting started  ')
    expect(searchCallCount).toBe(0)

    // 回车触发一次；body 恰为 trim 后的 query 与 channelId，不含 topK
    //（缺省口径与 MCP 一致，前端固定值会偏离同口径召回）。
    await user.keyboard('{Enter}')
    expect(await screen.findByTestId('search-result-list')).toBeInTheDocument()
    expect(searchCallCount).toBe(1)
    expect(capturedBodies[0]).toEqual({
      query: 'getting started',
      channelId: CHANNEL_ID,
    })

    // 按钮路径同样可用。
    await user.click(screen.getByTestId('search-submit-button'))
    expect(searchCallCount).toBe(2)
    expect(capturedBodies[1]).toEqual({
      query: 'getting started',
      channelId: CHANNEL_ID,
    })
  })

  it.each([
    ['isComposing (standard browsers)', { isComposing: true }],
    ['keyCode 229 (Safari composition end)', { keyCode: 229 }],
  ])(
    'does not search when Enter confirms an IME composition (%s)',
    (_label, keyInit) => {
      // WHY: 中文 IME 里按 Enter 是确认候选词而不是提交；无守卫会把组合中的
      // 文本当作关键词发出请求。
      installSearchHandler()
      render(<SearchBox channelId={CHANNEL_ID} />)

      const input = screen.getByTestId('search-input')
      fireEvent.change(input, { target: { value: '正在组合的中文' } })
      fireEvent.keyDown(input, { key: 'Enter', ...keyInit })

      expect(searchCallCount).toBe(0)
      expect(input).toHaveValue('正在组合的中文')
    },
  )

  it.each([
    ['empty string', ''],
    ['spaces only', '   '],
    ['tabs only', '\t\t'],
  ])('ignores submit and disables the button for blank input (%s)', async (_label, value) => {
    const user = userEvent.setup()
    installSearchHandler()
    render(<SearchBox channelId={CHANNEL_ID} />)

    if (value !== '') {
      fireEvent.change(screen.getByTestId('search-input'), {
        target: { value },
      })
    }
    // WHY: fireEvent.change 不移动焦点，Enter 必须在聚焦 input 后按出，
    // keydown 才会从 input 冒泡进组件根 div 的 onKeyDown——否则本用例
    // 检测不到 submit() 中 trimmed 守卫被删除的回归。
    screen.getByTestId('search-input').focus()
    await user.keyboard('{Enter}')

    // WHY: 空查询打到后端只会得到 400；按钮不可点让入口自明。
    expect(searchCallCount).toBe(0)
    expect(screen.getByTestId('search-submit-button')).toBeDisabled()
  })
})

describe('search result states', () => {
  it('shows loading while deferred, then renders items in the returned order even with non-monotonic scores', async () => {
    installDeferredSearch()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(userEvent.setup(), 'rust')
    expect(await screen.findByTestId('search-loading')).toBeInTheDocument()

    // 相邻 score 非单调 + 极低分条目：前端按返回顺序渲染，不重排、不过滤。
    const items = [
      makeItem({ title: '页 A 片段1', score: 0.9 }),
      makeItem({ title: '页 A 片段2', score: 0.02 }),
      makeItem({ title: '页 B 片段1', score: 0.95 }),
      makeItem({ title: '页 C 片段1', score: 0.0001 }),
    ]
    await act(async () => {
      pendingSearches[0](items)
    })

    expect(screen.queryByTestId('search-loading')).toBeNull()
    // 顺序与数量锁定：客户端按 score 重排（0.95 应居首）或低分过滤
    // （0.0001 被隐藏）都会让此断言失败。
    const titles = within(screen.getByTestId('search-result-list'))
      .getAllByTestId('search-result-item')
      .map((el) => el.querySelector('p')?.textContent)
    expect(titles).toEqual(items.map((item) => item.title))
  })

  it('renders the empty state (not an error) for zero-hit results', async () => {
    installSearchHandler(() => HttpResponse.json({ results: [] }))
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(userEvent.setup(), 'nohit')
    expect(await screen.findByTestId('search-empty')).toBeInTheDocument()
    // WHY: 空结果 = 检索零命中（正常结束），被当错误渲染会误导用户重试。
    expect(screen.queryByTestId('search-error')).toBeNull()
  })

  it('renders the 400 business message verbatim and recovers on a successful resubmit', async () => {
    server.use(
      http.post(SEARCH_URL, () =>
        HttpResponse.json(
          { code: 400, message: '频道 ghost 未配置' },
          { status: 400 },
        ),
      ),
    )
    const user = userEvent.setup()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(user, 'anything')
    expect(await screen.findByText('频道 ghost 未配置')).toBeInTheDocument()

    // 关键词保留可再次提交；成功后错误消失。
    installSearchHandler()
    await user.keyboard('{Enter}')
    expect(await screen.findByTestId('search-result-list')).toBeInTheDocument()
    expect(screen.queryByText('频道 ghost 未配置')).toBeNull()
  })

  it.each([
    ['500 response', () => HttpResponse.json({ code: 500, message: 'CoreError: 查询向量化失败: timeout' }, { status: 500 })],
    ['network failure', () => HttpResponse.error()],
  ])('shows the fixed failure copy without internal details (%s)', async (_label, respond) => {
    server.use(http.post(SEARCH_URL, () => respond()))
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(userEvent.setup(), 'anything')
    const error = await screen.findByTestId('search-error')
    expect(error).toHaveTextContent('搜索失败，请稍后重试')
    // WHY: 500 与网络失败的内部细节不外露，只呈现固定兜底文案。
    expect(screen.queryByText(/向量化失败/)).toBeNull()
  })

  it('renders linked items as new-tab anchors and unlinked items as non-interactive blocks', async () => {
    server.use(
      http.post(SEARCH_URL, () =>
        HttpResponse.json({
          results: [
            makeItem({ title: '有来源', link: 'https://a.example.com/x' }),
            makeItem({ title: '无来源', link: null }),
          ],
        }),
      ),
    )
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(userEvent.setup(), 'q')
    const items = await screen.findAllByTestId('search-result-item')

    const linked = within(items[0]).getByTestId('search-result-link')
    expect(linked).toHaveAttribute('href', 'https://a.example.com/x')
    // 新标签页打开来源，noopener 防止 window.opener 泄漏。
    expect(linked).toHaveAttribute('target', '_blank')
    expect(linked).toHaveAttribute('rel', 'noopener noreferrer')

    // 无链接条目：标题与摘要照常展示，但不是链接（不可点、不跳转）。
    expect(within(items[1]).getByText('无来源')).toBeInTheDocument()
    expect(within(items[1]).queryByRole('link')).toBeNull()
    expect(within(items[1]).getByText('暂无来源链接')).toBeInTheDocument()
  })
})

describe('in-flight requests and dismissal', () => {
  it('keeps only the newest result when submissions race (B resolves before A)', async () => {
    installDeferredSearch()
    render(<SearchBox channelId={CHANNEL_ID} />)

    const user = userEvent.setup()
    await submit(user, 'slow-a')
    fireEvent.change(screen.getByTestId('search-input'), {
      target: { value: 'fast-b' },
    })
    await user.keyboard('{Enter}')
    expect(searchCallCount).toBe(2)

    await act(async () => {
      pendingSearches[1]([makeItem({ title: 'B 的结果' })])
    })
    expect(await screen.findByText('B 的结果')).toBeInTheDocument()

    // 迟到的 A 响应被 cancelled 守卫丢弃；丢失守卫时旧结果会覆盖新结果。
    await act(async () => {
      pendingSearches[0]([makeItem({ title: 'A 的结果' })])
    })
    expect(screen.queryByText('A 的结果')).toBeNull()
    expect(screen.getByText('B 的结果')).toBeInTheDocument()
  })

  it('dedupes Enter while the same query is in flight; resubmits after it settles', async () => {
    installDeferredSearch()
    const user = userEvent.setup()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(user, 'same')
    expect(searchCallCount).toBe(1)
    await user.keyboard('{Enter}')
    // WHY: 连按回车不得成倍放大公开检索成本（每次含 LLM 改写 + embedding）。
    expect(searchCallCount).toBe(1)

    await act(async () => {
      pendingSearches[0]([makeItem()])
    })
    expect(await screen.findByTestId('search-result-list')).toBeInTheDocument()

    await user.keyboard('{Enter}')
    expect(searchCallCount).toBe(2)
  })

  it('drops the dropdown and the in-flight result when the input is cleared', async () => {
    installDeferredSearch()
    const user = userEvent.setup()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(user, 'doomed')
    expect(await screen.findByTestId('search-loading')).toBeInTheDocument()
    fireEvent.change(screen.getByTestId('search-input'), {
      target: { value: '' },
    })
    expect(screen.queryByTestId('search-dropdown')).toBeNull()

    // 响应到达后不再出现：清空即放弃，旧结果不能「复活」。
    await act(async () => {
      pendingSearches[0]([makeItem({ title: '不应出现' })])
    })
    expect(screen.queryByText('不应出现')).toBeNull()
    expect(screen.queryByTestId('search-dropdown')).toBeNull()
  })

  it('collapses the shown results on clear without a new request', async () => {
    installSearchHandler()
    const user = userEvent.setup()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(user, 'hit')
    expect(await screen.findByTestId('search-result-list')).toBeInTheDocument()
    fireEvent.change(screen.getByTestId('search-input'), {
      target: { value: '' },
    })
    expect(screen.queryByTestId('search-dropdown')).toBeNull()
    expect(searchCallCount).toBe(1)
  })

  it('collapses on Esc (focus back to input) and outside pointerdown, reopens on input click without new requests, and keeps open on result link click', async () => {
    installSearchHandler(() =>
      HttpResponse.json({ results: [makeItem({ title: '结果条目' })] }),
    )
    const user = userEvent.setup()
    render(<SearchBox channelId={CHANNEL_ID} />)

    await submit(user, 'q')
    expect(await screen.findByTestId('search-result-list')).toBeInTheDocument()

    // 点击结果链接属于区域内操作，不收起（否则链接点击会被误伤）。
    fireEvent.click(screen.getByTestId('search-result-link'))
    expect(screen.getByTestId('search-dropdown')).toBeInTheDocument()

    // Esc 收起且焦点回到输入框（键盘用户可立即改词重搜）。
    const input = screen.getByTestId('search-input')
    fireEvent.keyDown(input, { key: 'Escape' })
    expect(screen.queryByTestId('search-dropdown')).toBeNull()
    expect(input).toHaveFocus()

    // 点击输入框重开且不重新检索。
    fireEvent.click(input)
    expect(screen.getByTestId('search-dropdown')).toBeInTheDocument()
    expect(searchCallCount).toBe(1)

    // 区域外 pointerdown 收起（点击聊天按钮等区域外元素同理）。
    fireEvent.pointerDown(document.body)
    expect(screen.queryByTestId('search-dropdown')).toBeNull()
    expect(searchCallCount).toBe(1)
  })
})
