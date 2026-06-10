import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { useChatStreamContext } from '@/components/chat/chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import { ChatPanel } from '@/components/chat/chat-panel'

vi.mock('@/components/chat/chat-stream-context', () => ({
  useChatStreamContext: vi.fn(),
}))

beforeAll(() => {
  Element.prototype.scrollIntoView = vi.fn()
})

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  vi.mocked(useChatStreamContext).mockReturnValue({
    sendMessage: vi.fn(),
    stopStreaming: vi.fn(),
  } as never)
  useChatStore.setState({
    messages: [],
    sessionId: null,
    updatedAt: null,
    isLoading: false,
    error: null,
  })
})

function renderPanel() {
  return render(<ChatPanel />)
}

describe('ChatPanel error display', () => {
  it('does not show error banner when store has no error', () => {
    useChatStore.setState({ error: null })

    renderPanel()

    expect(screen.queryByTestId('chat-error-banner')).not.toBeInTheDocument()
  })
})

describe('ChatPanel clear conversation', () => {
  it('shows clear button when there is an active conversation', () => {
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
      updatedAt: Date.now(),
    })

    renderPanel()

    expect(screen.getByTestId('chat-clear-button')).toBeInTheDocument()
  })

  it('clears messages and session when clear button is clicked', async () => {
    const user = userEvent.setup()
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
      updatedAt: Date.now(),
    })

    renderPanel()

    await user.click(screen.getByTestId('chat-clear-button'))

    const state = useChatStore.getState()
    expect(state.messages).toEqual([])
    expect(state.sessionId).toBeNull()
    expect(screen.queryByTestId('chat-clear-button')).not.toBeInTheDocument()
  })

  it('stops streaming before clearing an in-progress conversation', async () => {
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
      updatedAt: Date.now(),
      isLoading: true,
    })

    renderPanel()

    await user.click(screen.getByTestId('chat-clear-button'))

    expect(stopStreaming).toHaveBeenCalledTimes(1)
    expect(useChatStore.getState().messages).toEqual([])
  })
})
