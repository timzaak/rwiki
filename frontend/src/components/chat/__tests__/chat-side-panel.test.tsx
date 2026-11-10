import { render, screen, act, fireEvent } from '@testing-library/react'

import { useChatStreamContext } from '@/components/chat/chat-stream-context'
import { useChatModalStore, useChatStore } from '@/stores/chat-store'
import { ChatSidePanel } from '@/components/chat/chat-side-panel'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'

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
  useChatModalStore.setState({ isModalOpen: false })
})

function renderPanel(props: Partial<Parameters<typeof ChatSidePanel>[0]> = {}) {
  return render(
    <ChannelIdProvider channelId={['test-channel']}>
      <ChatSidePanel {...props} />
    </ChannelIdProvider>,
  )
}

function openPanel() {
  act(() => {
    useChatModalStore.setState({ isModalOpen: true })
  })
}

describe('ChatSidePanel open (US-CORE-047 scenario 1)', () => {
  it('renders the docked panel with the chat body when open', () => {
    renderPanel()
    expect(screen.queryByTestId('chat-side-panel')).not.toBeInTheDocument()

    openPanel()

    // The panel is a shell around the same ChatPanel the modal uses.
    expect(screen.getByTestId('chat-side-panel')).toBeInTheDocument()
    expect(screen.getByTestId('chat-side-panel-header')).toBeInTheDocument()
    expect(screen.getByTestId('chat-panel')).toBeInTheDocument()
  })

  it('renders no overlay node — the host page must stay visible and interactive', () => {
    // The modal shell renders a dimmed overlay as a sibling; the side panel
    // pushes content instead. A second root node here
    // would reintroduce modal semantics; click-through itself is asserted by
    // the e2e demo.
    renderPanel()
    openPanel()

    // The panel's direct parent is the render container: the component must
    // contribute exactly one root node (the modal contributes two — overlay
    // plus panel).
    const panelHost = screen.getByTestId('chat-side-panel').parentElement!
    expect(panelHost.childElementCount).toBe(1)
  })
})

describe('ChatSidePanel close (US-CORE-047 scenario 1: header close icon)', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    vi.runOnlyPendingTimers()
    vi.useRealTimers()
  })

  it('close icon closes the shared state, keeps the exit animation, then unmounts', () => {
    renderPanel()
    openPanel()

    // fireEvent (not userEvent): the click itself must not depend on timers —
    // userEvent's internal delay timer hangs under fake timers here.
    fireEvent.click(screen.getByTestId('chat-side-panel-close'))

    // The shared switch flips at once (floating button comes back).
    expect(useChatModalStore.getState().isModalOpen).toBe(false)
    // The panel itself stays mounted through the 300ms slide-out.
    expect(screen.getByTestId('chat-side-panel')).toBeInTheDocument()

    act(() => {
      vi.advanceTimersByTime(300)
    })

    expect(screen.queryByTestId('chat-side-panel')).not.toBeInTheDocument()
  })

  it('reopening during the slide-out cancels the unmount', () => {
    renderPanel()
    openPanel()

    fireEvent.click(screen.getByTestId('chat-side-panel-close'))
    openPanel()

    act(() => {
      vi.advanceTimersByTime(300)
    })

    // The conversation view must survive a rapid toggle — conversation state
    // itself lives in the store and is asserted by the e2e story.
    expect(screen.getByTestId('chat-side-panel')).toBeInTheDocument()
    expect(useChatModalStore.getState().isModalOpen).toBe(true)
  })
})

describe('ChatSidePanel props pass-through (US-CORE-047 scenario 2 semantics)', () => {
  it('shows the welcome message and suggested questions inside the panel', () => {
    renderPanel({
      welcomeMessage: 'Welcome to the docs',
      suggestedQuestions: ['How to get started'],
    })
    openPanel()

    expect(screen.getByText('Welcome to the docs')).toBeInTheDocument()
    expect(screen.getByTestId('suggested-question-button')).toHaveTextContent(
      'How to get started',
    )
  })
})
