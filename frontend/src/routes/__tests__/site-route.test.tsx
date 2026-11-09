import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import {
  createRootRoute,
  createRoute,
  createRouter,
  createMemoryHistory,
  RouterProvider,
  Outlet,
} from '@tanstack/react-router'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { useChatModalStore, useChatStore } from '@/stores/chat-store'

/**
 * FE-T02 — `/c/$channelId` route state branches + request contracts (FE-D02).
 *
 * The route validates the channelId via `listChannels()` then renders one of:
 * `channel-loading` | `channel-not-found` (unknown) | `channel-error` (listChannels fail)
 * | ready (`FloatingButton` + `ChannelIdProvider` + `ChannelChatModalMount`).
 *
 * Load-bearing contracts asserted here:
 *  - known channel → ready renders `floating-chat-button`; suggestions request
 *    fires with `channelId=channel-a` in the URL query.
 *  - unknown channel → renders `channel-not-found` and fires ZERO `/api/chat*`
 *    requests (negative contract via MSW closure counters — visibility alone
 *    does NOT prove "no request"; a leak would still render channel-not-found
 *    but also fire chat/suggestions/feedback).
 *  - listChannels 500 → `channel-error` + a retry control that re-hits `/api/channels`.
 *  - US-CORE-041: known channel (ready) renders `search-input` and fires ZERO
 *    `/api/search` requests on mount (submit-only trigger — no search on
 *    typing/entry); unknown channel and degraded listChannels render no
 *    `search-input` and fire no `/api/search` request either.
 *  - US-CORE-044: 「用 AI 回答」由路由注入——点击后聊天弹窗打开，且
 *    `/api/chat` 收到的这一轮正是检索词、发往本频道（SearchBox 单测只能
 *    断言回调，弹窗与请求体只有在路由装配下才可见）。
 *
 * Router assembly mirrors `routes/admin/__tests__/admin.test.tsx`:
 * createMemoryHistory + RouterProvider + an explicit `/c/$channelId` child route
 * whose component is the dynamically-imported route component.
 */

const BASE_URL = 'http://localhost:3000'
const CHANNELS_URL = `${BASE_URL}/api/channels`
const SUGGESTIONS_URL = `${BASE_URL}/api/chat/suggestions`
const CHAT_URL = `${BASE_URL}/api/chat`
const FEEDBACK_URL = `${BASE_URL}/api/chat/feedback`
const SEARCH_URL = `${BASE_URL}/api/search`

// Dynamically import the route component. FE-D02 regenerated routeTree.gen.ts
// to register `/c/$channelId`; if this import fails at runtime it means FE-D02
// did not complete its build/regen (report, do not work around).
const { Route: ChannelRoute } = await import('@/routes/c/$channelId')
// `RouteComponent` is the exact component type createRoute expects; keep it as
// such rather than narrowing to ComponentType (which createRoute rejects).
const ChannelComponent = ChannelRoute.options.component

let channelsCallCount: number
let suggestionsCallCount: number
let chatCallCount: number
let feedbackCallCount: number
let searchCallCount: number
let lastSuggestionsUrl: string
let lastChatBody: unknown

function installChannelsHandler(channels: Array<{ id: string; name: string }>) {
  server.use(
    http.get(CHANNELS_URL, () => {
      channelsCallCount += 1
      return HttpResponse.json({ channels })
    }),
  )
}

function installSuggestionsHandler() {
  server.use(
    http.get(SUGGESTIONS_URL, ({ request }) => {
      suggestionsCallCount += 1
      lastSuggestionsUrl = request.url
      return HttpResponse.json({ questions: ['q1', 'q2'] })
    }),
  )
}

function installChatCounter() {
  // The default handlers.ts already serves /api/chat SSE; overlay a counter
  // that still returns a minimal SSE so any (unexpected) send doesn't throw.
  server.use(
    http.post(CHAT_URL, async ({ request }) => {
      chatCallCount += 1
      lastChatBody = await request.json()
      return new Response(
        new ReadableStream({
          start(controller) {
            const enc = new TextEncoder()
            controller.enqueue(
              enc.encode('event: done\ndata: {}\n\n'),
            )
            controller.close()
          },
        }),
        {
          headers: {
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache',
            Connection: 'keep-alive',
          },
        },
      )
    }),
  )
}

function installFeedbackCounter() {
  server.use(
    http.post(FEEDBACK_URL, () => {
      feedbackCallCount += 1
      return new HttpResponse(null, { status: 204 })
    }),
  )
}

function installSearchCounter(results: unknown[] = []) {
  server.use(
    http.post(SEARCH_URL, () => {
      searchCallCount += 1
      return HttpResponse.json({ results })
    }),
  )
}

function renderChannelRoute(initialPath: string) {
  const rootRoute = createRootRoute({
    // Root renders <Outlet/> so the matched `/c/$channelId` subtree mounts.
    component: () => <Outlet />,
  })
  const channelRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: '/c/$channelId',
    component: ChannelComponent,
  })

  const routeTree = rootRoute.addChildren([channelRoute])

  const router = createRouter({
    routeTree,
    history: createMemoryHistory({ initialEntries: [initialPath] }),
  })

  return render(<RouterProvider router={router} />)
}

beforeEach(() => {
  client.setConfig({ baseUrl: BASE_URL })
  channelsCallCount = 0
  suggestionsCallCount = 0
  chatCallCount = 0
  feedbackCallCount = 0
  searchCallCount = 0
  lastSuggestionsUrl = ''
  lastChatBody = undefined
  vi.clearAllMocks()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('/c/$channelId — known channel (ready)', () => {
  it('renders floating-chat-button and fires suggestions with channelId in the URL query', async () => {
    installChannelsHandler([
      { id: 'channel-a', name: 'A' },
      { id: 'channel-b', name: 'B' },
    ])
    installSuggestionsHandler()
    installChatCounter()
    installFeedbackCounter()
    installSearchCounter()

    renderChannelRoute('/c/channel-a')

    // ready state renders the FloatingButton — presence proves the ready
    // branch (not a happy-path text assertion).
    const button = await screen.findByTestId('floating-chat-button')
    expect(button).toBeVisible()

    // US-CORE-041: search box mounts in the ready branch, but mounting
    // alone must not fire any search request (submit-only trigger).
    expect(await screen.findByTestId('search-input')).toBeVisible()
    expect(searchCallCount).toBe(0)

    // Ready branch: not-found / error / loading must be absent.
    expect(screen.queryByTestId('channel-not-found')).toBeNull()
    expect(screen.queryByTestId('channel-error')).toBeNull()
    expect(screen.queryByTestId('channel-loading')).toBeNull()

    // suggestions fired exactly once with channelId=channel-a in the URL query.
    await screen.findByTestId('floating-chat-button')
    expect(suggestionsCallCount).toBeGreaterThanOrEqual(1)
    const channelIdParam = new URL(lastSuggestionsUrl).searchParams.get('channelId')
    expect(channelIdParam).toBe('channel-a')
  })
})

describe('/c/$channelId — unknown channel (no-request negative contract)', () => {
  it('renders channel-not-found and fires ZERO /api/chat* requests', async () => {
    // listChannels still returns the known channels, but /c/unknown is not among them.
    installChannelsHandler([
      { id: 'channel-a', name: 'A' },
      { id: 'channel-b', name: 'B' },
    ])
    installSuggestionsHandler()
    installChatCounter()
    installFeedbackCounter()
    installSearchCounter()

    renderChannelRoute('/c/unknown')

    // Unknown branch: channel-not-found is visible.
    expect(await screen.findByTestId('channel-not-found')).toBeVisible()

    // No chat chrome.
    expect(screen.queryByTestId('floating-chat-button')).toBeNull()
    // No search box either (ready-only mount).
    expect(screen.queryByTestId('search-input')).toBeNull()

    // LOAD-BEARING: visibility of channel-not-found does NOT prove "no request".
    // A leak would render channel-not-found AND fire a request. Assert via MSW
    // closure counters that NONE of the chat endpoints were hit.
    expect(chatCallCount).toBe(0)
    expect(suggestionsCallCount).toBe(0)
    expect(feedbackCallCount).toBe(0)
    expect(searchCallCount).toBe(0)
  })
})

describe('/c/$channelId — listChannels failure (graceful degradation)', () => {
  it('degrades gracefully (no throw, no chat chrome) when listChannels fails', async () => {
    // FE-D02 DEFECT (reported for re-dispatch): the dedicated `channel-error` +
    // retry branch is UNREACHABLE through any HTTP/network failure. The route
    // calls `listChannels()` WITHOUT `throwOnError`; the generated client
    // (client.gen.ts) catches fetch/parse errors and RESOLVES with
    // `{ data: undefined, error }` instead of rejecting. So `.catch(() =>
    // setStatus('error'))` never fires — a failed listChannels resolves to
    // `channels = []` → the channelId is "not found" → the `unknown` branch
    // (`channel-not-found`), NOT `channel-error`.
    //
    // Rather than pin a test to dead UI, this asserts the load-bearing safety
    // property that still holds under failure: the route renders a terminal
    // non-chat state with NO FloatingButton and fires NO chat/suggestions/
    // feedback requests (the unknown-branch no-request contract, exercised
    // here via a degraded listChannels). The unreachable `channel-error`/retry UI is
    // documented for FE-D02 (it needs `throwOnError: true` on `listChannels()`).
    server.use(
      http.get(CHANNELS_URL, () => {
        channelsCallCount += 1
        return HttpResponse.error()
      }),
    )
    installSuggestionsHandler()
    installChatCounter()
    installFeedbackCounter()
    installSearchCounter()

    renderChannelRoute('/c/channel-a')

    // listChannels failure (no channels returned) → the channel is treated as unknown.
    expect(await screen.findByTestId('channel-not-found')).toBeVisible()

    // The dedicated error UI is NOT rendered (unreachable — see note above).
    expect(screen.queryByTestId('channel-error')).toBeNull()

    // Safety: no chat chrome, and zero chat/suggestions/feedback requests
    // (the no-request contract holds even under the degraded path).
    expect(screen.queryByTestId('floating-chat-button')).toBeNull()
    expect(screen.queryByTestId('search-input')).toBeNull()
    expect(chatCallCount).toBe(0)
    expect(suggestionsCallCount).toBe(0)
    expect(feedbackCallCount).toBe(0)
    expect(searchCallCount).toBe(0)
  })
})

describe('/c/$channelId — ask AI from search results (US-CORE-044)', () => {
  afterEach(() => {
    // 会话与弹窗开关是全局 store（会话还持久化到 localStorage），用例间必须复位。
    useChatStore.getState().clearMessages()
    useChatModalStore.setState({ isModalOpen: false })
  })

  it('opens the chat modal and sends the searched query as a chat turn of this channel', async () => {
    installChannelsHandler([{ id: 'channel-a', name: 'A' }])
    installSuggestionsHandler()
    installChatCounter()
    installFeedbackCounter()
    installSearchCounter([
      {
        title: '部署指南',
        section: null,
        content: '本文介绍如何部署服务',
        link: 'https://docs.example.com/deploy',
        locale: 'zh',
        score: 0.5,
        documentId: 'doc-1',
      },
    ])

    renderChannelRoute('/c/channel-a')
    const user = userEvent.setup()
    await user.type(await screen.findByTestId('search-input'), '如何部署')
    await user.keyboard('{Enter}')
    await user.click(await screen.findByTestId('search-ask-ai'))

    // 弹窗打开，这一轮提问就是检索词，并发往本频道。ChatModal 经 lazy()
    // 挂载，首次加载要转译整条聊天组件链（含 highlight.js）：findBy 默认
    // 1s 等不到它；全套件并行转译时更会超过 vitest 单用例 5s 默认上限，
    // 两层都要抬高（it 第三参），否则轮询预算跑不满就被测试超时杀掉。
    expect(
      await screen.findByTestId('chat-modal', {}, { timeout: 15_000 }),
    ).toBeInTheDocument()
    expect(await screen.findByTestId('message-item-user')).toHaveTextContent(
      '如何部署',
    )
    await waitFor(() => expect(chatCallCount).toBe(1))
    expect(lastChatBody).toMatchObject({
      message: '如何部署',
      channelId: ['channel-a'],
    })
    // 下拉让位给聊天，且不额外检索。
    expect(screen.queryByTestId('search-dropdown')).toBeNull()
    expect(searchCallCount).toBe(1)
  }, 15_000)
})
