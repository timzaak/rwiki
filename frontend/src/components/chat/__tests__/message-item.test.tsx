import { render, screen } from '@testing-library/react'

import type { ChatMessage } from '@/stores/chat-store'
import { MessageItem } from '@/components/chat/message-item'

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
