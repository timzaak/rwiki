import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { useChatStreamContext } from '@/components/chat/chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import {
  useChatHistoryErrorStore,
  useChatHistoryStore,
} from '@/stores/chat-history-store'
import { ChatPanel } from '@/components/chat/chat-panel'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'

vi.mock('@/components/chat/chat-stream-context', () => ({
  useChatStreamContext: vi.fn(),
}))

// isHistoryStorageAvailable 在模块内做一次性探测并缓存，无法按用例切换真
// 实实现；用可变 mock 委托，保留其余导出（startNewConversation 等）为真实实现。
const mockStorageAvailable = vi.fn(() => true)
vi.mock('@/stores/chat-history-store', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('@/stores/chat-history-store')>()
  return {
    ...actual,
    isHistoryStorageAvailable: () => mockStorageAvailable(),
  }
})

beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn()
})

beforeEach(() => {
  vi.clearAllMocks()
  mockStorageAvailable.mockReturnValue(true)
  localStorage.clear()
  vi.mocked(useChatStreamContext).mockReturnValue({
    sendMessage: vi.fn(),
    stopStreaming: vi.fn(),
  } as never)
  useChatStore.setState({
    messages: [],
    sessionId: null,
    conversationId: null,
    channelKey: null,
    updatedAt: null,
    isLoading: false,
    error: null,
  })
  useChatHistoryStore.setState({ sessions: [] })
  useChatHistoryErrorStore.setState({ storageError: false })
})

function renderPanel() {
  return render(
    <ChannelIdProvider channelId={['test-channel']}>
      <ChatPanel />
    </ChannelIdProvider>,
  )
}

function seedActiveConversation() {
  useChatStore.setState({
    messages: [
      {
        id: 'msg-1',
        role: 'user',
        content: 'Hello',
        timestamp: Date.now(),
      },
    ],
    sessionId: 'session-1',
    conversationId: 'conv-1',
    updatedAt: Date.now(),
  })
}

describe('ChatPanel error display', () => {
  it('does not show error banner when store has no error', () => {
    useChatStore.setState({ error: null })

    renderPanel()

    expect(screen.queryByTestId('chat-error-banner')).not.toBeInTheDocument()
  })
})

describe('ChatPanel new conversation', () => {
  it('shows the new conversation button only when there is an active conversation', () => {
    const { unmount } = renderPanel()
    expect(
      screen.queryByTestId('chat-new-conversation-button'),
    ).not.toBeInTheDocument()
    unmount()

    seedActiveConversation()
    renderPanel()
    expect(
      screen.getByTestId('chat-new-conversation-button'),
    ).toBeInTheDocument()
  })

  it('archives the current conversation and starts an empty one on click', async () => {
    // WHY (US-CORE-048 场景 2): "新对话"的语义是归档而非销毁——旧问答必须
    // 完整进历史列表，否则用户一点按钮就永久丢失刚问过的内容。
    const user = userEvent.setup()
    seedActiveConversation()
    renderPanel()

    await user.click(screen.getByTestId('chat-new-conversation-button'))

    const workspace = useChatStore.getState()
    expect(workspace.messages).toEqual([])
    expect(workspace.sessionId).toBeNull()
    expect(workspace.conversationId).toBeNull()

    const sessions = useChatHistoryStore.getState().sessions
    expect(sessions).toHaveLength(1)
    expect(sessions[0]).toMatchObject({
      id: 'conv-1',
      sessionId: 'session-1',
      title: 'Hello',
    })

    expect(
      screen.queryByTestId('chat-new-conversation-button'),
    ).not.toBeInTheDocument()
  })

  it('stops streaming before archiving an in-progress conversation', async () => {
    // WHY: 流式中点新对话，先把在途回答按打断语义收尾再归档——否则归档快照
    // 里会留下永远 isStreaming 的消息。
    const user = userEvent.setup()
    const stopStreaming = vi.fn()
    vi.mocked(useChatStreamContext).mockReturnValue({
      sendMessage: vi.fn(),
      stopStreaming,
    } as never)
    useChatStore.setState({
      messages: [
        {
          id: 'asst-1',
          role: 'assistant',
          content: 'Partial',
          timestamp: Date.now(),
          isStreaming: true,
        },
      ],
      sessionId: 'session-1',
      conversationId: 'conv-1',
      updatedAt: Date.now(),
      isLoading: true,
    })

    renderPanel()

    await user.click(screen.getByTestId('chat-new-conversation-button'))

    expect(stopStreaming).toHaveBeenCalledTimes(1)
    expect(useChatStore.getState().messages).toEqual([])
    expect(useChatHistoryStore.getState().sessions).toHaveLength(1)
  })
})

describe('ChatPanel history entry', () => {
  it('renders the history toggle when local storage is available', () => {
    mockStorageAvailable.mockReturnValue(true)

    renderPanel()

    expect(screen.getByTestId('chat-history-toggle-button')).toBeInTheDocument()
  })

  it('hides the history toggle when local storage is unavailable', () => {
    // WHY (US-CORE-049 场景 3): 受限环境（opaque origin）下聊天必须照常可用、
    // 只降级掉历史入口——出现一个点了没反应的按钮比没有按钮更糟。
    mockStorageAvailable.mockReturnValue(false)

    renderPanel()

    expect(
      screen.queryByTestId('chat-history-toggle-button'),
    ).not.toBeInTheDocument()
    expect(screen.queryByTestId('chat-history-panel')).not.toBeInTheDocument()
  })

  it('opens and closes the history panel via the toggle', async () => {
    const user = userEvent.setup()
    renderPanel()

    await user.click(screen.getByTestId('chat-history-toggle-button'))
    expect(screen.getByTestId('chat-history-panel')).toBeInTheDocument()

    await user.click(screen.getByTestId('chat-history-toggle-button'))
    expect(screen.queryByTestId('chat-history-panel')).not.toBeInTheDocument()
  })
})
