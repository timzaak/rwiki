import { render, screen } from '@testing-library/react'

import type { ChatMessage } from '@/stores/chat-store'
import { useChatStore } from '@/stores/chat-store'
import { MessageItem } from '@/components/chat/message-item'
import { client } from '@/lib/api-generated/client.gen'

function makeMessage(overrides?: Partial<ChatMessage>): ChatMessage {
  return {
    id: 'msg-1',
    role: 'user',
    content: 'Hello',
    timestamp: Date.now(),
    ...overrides,
  }
}

function renderMessage(overrides?: Partial<ChatMessage>) {
  return render(<MessageItem message={makeMessage(overrides)} />)
}

describe('MessageItem conditional rendering', () => {
  it('shows streaming cursor when message is streaming', () => {
    renderMessage({ isStreaming: true })

    expect(screen.getByTestId('message-item-streaming')).toBeInTheDocument()
  })

  it('does not show streaming cursor when message is not streaming', () => {
    renderMessage({ isStreaming: false })

    expect(screen.queryByTestId('message-item-streaming')).not.toBeInTheDocument()
  })

  it('shows error indicator when message has error', () => {
    renderMessage({ error: 'Stream interrupted' })

    expect(screen.getByText('Stream interrupted')).toBeInTheDocument()
  })

  it.each(['user', 'assistant'] as const)(
    'renders with correct role testid for %s',
    (role) => {
      renderMessage({ role })

      expect(screen.getByTestId(`message-item-${role}`)).toBeInTheDocument()
    },
  )
})

describe('MessageItem feedback buttons', () => {
  beforeEach(() => {
    // The hook needs a valid sessionId and store messages for userMessage lookup.
    // Seed the store with a user message before the assistant message so that
    // useFeedback can find the preceding user message content.
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'msg-user-1', role: 'user', content: 'User question' }),
        makeMessage({
          id: 'msg-asst-1',
          role: 'assistant',
          content: 'AI response',
          isStreaming: false,
        }),
      ],
      sessionId: 'test-session',
      isLoading: false,
      error: null,
    })
    // SDK client needs absolute URL in MSW/Node.js environment
    client.setConfig({ baseUrl: 'http://localhost:3000' })
  })

  it('shows feedback buttons for assistant message when not streaming', () => {
    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'AI response',
      isStreaming: false,
    })

    expect(screen.getByTestId('feedback-like-button')).toBeInTheDocument()
    expect(screen.getByTestId('feedback-dislike-button')).toBeInTheDocument()
  })

  it('hides feedback buttons for user message', () => {
    renderMessage({
      id: 'msg-user-1',
      role: 'user',
      content: 'User question',
    })

    expect(screen.queryByTestId('feedback-like-button')).not.toBeInTheDocument()
    expect(screen.queryByTestId('feedback-dislike-button')).not.toBeInTheDocument()
  })

  it('hides feedback buttons during streaming', () => {
    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'AI response',
      isStreaming: true,
    })

    expect(screen.queryByTestId('feedback-like-button')).not.toBeInTheDocument()
    expect(screen.queryByTestId('feedback-dislike-button')).not.toBeInTheDocument()
  })
})
