import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { ChatHistoryPanel } from '@/components/chat/chat-history-panel'
import { useChatStore } from '@/stores/chat-store'
import {
  useChatHistoryErrorStore,
  useChatHistoryStore,
  type HistorySession,
} from '@/stores/chat-history-store'

function makeSession(overrides?: Partial<HistorySession>): HistorySession {
  return {
    id: 'conv-archived',
    channelKey: '["help_center"]',
    sessionId: 'session-archived',
    title: 'Archived question',
    messages: [
      {
        id: 'user-1',
        role: 'user',
        content: 'Archived question',
        timestamp: 1000,
      },
      {
        id: 'asst-1',
        role: 'assistant',
        content: 'Archived answer',
        timestamp: 2000,
      },
    ],
    createdAt: 1000,
    updatedAt: 2000,
    ...overrides,
  }
}

function renderPanel(onClose = vi.fn()) {
  const user = userEvent.setup()
  return { user, onClose, ...render(<ChatHistoryPanel onClose={onClose} />) }
}

function seedCurrentConversation(overrides?: {
  conversationId?: string
  updatedAt?: number
}) {
  useChatStore.setState({
    messages: [
      { id: 'user-cur', role: 'user', content: 'Current question', timestamp: 3000 },
      { id: 'asst-cur', role: 'assistant', content: 'Current answer', timestamp: 3100 },
    ],
    sessionId: 'session-current',
    conversationId: overrides?.conversationId ?? 'conv-current',
    channelKey: '["help_center"]',
    updatedAt: overrides?.updatedAt ?? 3100,
    isLoading: false,
    error: null,
  })
}

beforeEach(() => {
  localStorage.clear()
  useChatStore.setState({
    messages: [],
    sessionId: null,
    conversationId: null,
    channelKey: '["help_center"]',
    updatedAt: null,
    isLoading: false,
    error: null,
  })
  useChatHistoryStore.setState({ sessions: [] })
  useChatHistoryErrorStore.setState({ storageError: false })
})

describe('ChatHistoryPanel list rendering (US-CORE-048)', () => {
  it('shows archived sessions of the current channel, most recently active first', () => {
    useChatHistoryStore.setState({
      sessions: [
        makeSession({ id: 'conv-old', title: 'Older question', updatedAt: 1000 }),
        makeSession({ id: 'conv-new', title: 'Newer question', updatedAt: 4000 }),
        makeSession({
          id: 'conv-other',
          title: 'Other channel question',
          channelKey: '["developer_docs"]',
          updatedAt: 5000,
        }),
      ],
    })

    renderPanel()

    // WHY (US-CORE-049): 另一频道的会话绝不能出现在当前频道列表里。
    const titles = screen
      .getAllByTestId('chat-history-item-title')
      .map((el) => el.textContent)
    expect(titles).toEqual(['Newer question', 'Older question'])
  })

  it('highlights the current conversation without offering a delete button', () => {
    // WHY (US-CORE-051): 正在进行的对话只能经"新对话归档后再删除"两步销毁，
    // 列表内直接删除会把用户正在看的对话撕掉。
    seedCurrentConversation()
    useChatHistoryStore.setState({ sessions: [makeSession()] })

    renderPanel()

    const currentItem = screen
      .getAllByTestId('chat-history-item')
      .find((el) => el.getAttribute('aria-current') === 'true')
    expect(currentItem).toBeDefined()
    expect(currentItem!.textContent).toContain('Current question')
    expect(screen.getAllByTestId('chat-history-item-delete-button')).toHaveLength(1)
  })

  it('shows the empty state when there is nothing to show', () => {
    renderPanel()

    expect(screen.getByTestId('chat-history-empty')).toBeInTheDocument()
    expect(
      screen.queryByTestId('chat-history-clear-all-button'),
    ).not.toBeInTheDocument()
  })

  it('shows an inline notice when archive writes keep failing', () => {
    // Real quota scenario: construct a genuinely failing persist write — a
    // manually set flag would be cleared by its own successful write (the
    // notice auto-clears on the next successful write by design).
    const originalSetItem = Storage.prototype.setItem
    const setItemSpy = vi
      .spyOn(Storage.prototype, 'setItem')
      .mockImplementation(function (
        this: Storage,
        key: string,
        value: string,
      ) {
        if (key === 'rwiki-chat-history') {
          throw new DOMException('Quota exceeded', 'QuotaExceededError')
        }
        return originalSetItem.call(this, key, value)
      })

    try {
      useChatHistoryStore.setState({ sessions: [makeSession()] })

      renderPanel()

      expect(screen.getByTestId('chat-history-storage-error')).toBeInTheDocument()
    } finally {
      setItemSpy.mockRestore()
    }
  })
})

describe('ChatHistoryPanel switching (US-CORE-048)', () => {
  it('loads an archived session into the workspace and collapses on click', async () => {
    // WHY: 点击列表项必须立即回看完整问答并回到对话视图——列表挡住消息区，
    // 不收起就等于没切换。
    useChatHistoryStore.setState({ sessions: [makeSession()] })
    const { user, onClose } = renderPanel()

    await user.click(screen.getByTestId('chat-history-item-title'))

    const workspace = useChatStore.getState()
    expect(workspace.conversationId).toBe('conv-archived')
    expect(workspace.sessionId).toBe('session-archived')
    expect(workspace.messages.map((m) => m.content)).toEqual([
      'Archived question',
      'Archived answer',
    ])
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('collapses on Escape', async () => {
    const { user, onClose } = renderPanel()

    await user.keyboard('{Escape}')

    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('clicking the current conversation entry only collapses the list', async () => {
    seedCurrentConversation()
    const { user, onClose } = renderPanel()

    const currentItem = screen
      .getAllByTestId('chat-history-item')
      .find((el) => el.getAttribute('aria-current') === 'true')!
    await user.click(currentItem)

    expect(onClose).toHaveBeenCalledTimes(1)
    expect(useChatStore.getState().conversationId).toBe('conv-current')
  })
})

describe('ChatHistoryPanel deletion (US-CORE-051)', () => {
  it('deletes an archived session directly, without any confirmation step', async () => {
    // WHY: 删除单个会话是直接执行的业务规则（防误触只保护"清空全部"）。
    useChatHistoryStore.setState({
      sessions: [
        makeSession({ id: 'conv-1', title: 'First question' }),
        makeSession({ id: 'conv-2', title: 'Second question' }),
      ],
    })
    const { user } = renderPanel()

    const deleteButtons = screen.getAllByTestId('chat-history-item-delete-button')
    await user.click(deleteButtons[0])

    expect(screen.queryByTestId('dialog')).not.toBeInTheDocument()
    expect(
      useChatHistoryStore.getState().sessions.map((s) => s.id),
    ).toEqual(['conv-2'])
  })
})

describe('ChatHistoryPanel clear-all confirmation (US-CORE-051)', () => {
  it('keeps the history untouched when the confirmation is cancelled', async () => {
    useChatHistoryStore.setState({
      sessions: [makeSession({ id: 'conv-1' }), makeSession({ id: 'conv-2' })],
    })
    const { user } = renderPanel()

    await user.click(screen.getByTestId('chat-history-clear-all-button'))
    expect(screen.getByTestId('dialog')).toBeInTheDocument()

    await user.click(screen.getByTestId('dialog-cancel-button'))

    expect(screen.queryByTestId('dialog')).not.toBeInTheDocument()
    expect(useChatHistoryStore.getState().sessions).toHaveLength(2)
  })

  it('clears only the current channel archive and keeps the current conversation on confirm', async () => {
    // WHY (US-CORE-051): 清空范围 = 当前频道的历史归档；确认后当前对话必须
    // 原样留在聊天区域，否则清空变成了对正在进行的对话的突袭删除。
    seedCurrentConversation()
    useChatHistoryStore.setState({
      sessions: [
        makeSession({ id: 'conv-1' }),
        makeSession({
          id: 'conv-dev',
          channelKey: '["developer_docs"]',
        }),
      ],
    })
    const { user } = renderPanel()

    await user.click(screen.getByTestId('chat-history-clear-all-button'))
    await user.click(screen.getByTestId('dialog-submit-button'))

    expect(useChatHistoryStore.getState().sessions.map((s) => s.id)).toEqual([
      'conv-dev',
    ])
    const workspace = useChatStore.getState()
    expect(workspace.conversationId).toBe('conv-current')
    expect(workspace.messages).toHaveLength(2)
  })

  it('focuses the cancel button when the dialog opens so Enter cannot confirm by accident', async () => {
    useChatHistoryStore.setState({ sessions: [makeSession()] })
    const { user } = renderPanel()

    await user.click(screen.getByTestId('chat-history-clear-all-button'))

    expect(screen.getByTestId('dialog-cancel-button')).toHaveFocus()
  })

  it('Escape cancels the dialog without closing the list or clearing history', async () => {
    useChatHistoryStore.setState({ sessions: [makeSession()] })
    const { user, onClose } = renderPanel()

    await user.click(screen.getByTestId('chat-history-clear-all-button'))
    await user.keyboard('{Escape}')

    expect(screen.queryByTestId('dialog')).not.toBeInTheDocument()
    expect(screen.getByTestId('chat-history-panel')).toBeInTheDocument()
    expect(onClose).not.toHaveBeenCalled()
    expect(useChatHistoryStore.getState().sessions).toHaveLength(1)
  })
})
