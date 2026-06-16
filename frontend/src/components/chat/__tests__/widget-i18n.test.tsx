import { render, screen } from '@testing-library/react'

import { ChatInput } from '@/components/chat/chat-input'
import { WidgetI18nProvider } from '@/components/chat/widget-i18n'
import { resolveWidgetMessages } from '@/components/chat/messages'
import { useChatStreamContext } from '@/components/chat/chat-stream-context'

vi.mock('@/components/chat/chat-stream-context', () => ({
  useChatStreamContext: vi.fn(),
}))

beforeEach(() => {
  vi.mocked(useChatStreamContext).mockReturnValue({
    sendMessage: vi.fn(),
    stopStreaming: vi.fn(),
  } as never)
})

describe('WidgetI18nProvider', () => {
  it('renders the localized input placeholder for zh-CN', () => {
    render(
      <WidgetI18nProvider messages={resolveWidgetMessages('zh-CN')}>
        <ChatInput />
      </WidgetI18nProvider>,
    )

    expect(screen.getByTestId('chat-input')).toHaveAttribute(
      'placeholder',
      '随便问点什么…',
    )
  })

  it('renders the English input placeholder by default', () => {
    render(
      <WidgetI18nProvider messages={resolveWidgetMessages('en')}>
        <ChatInput />
      </WidgetI18nProvider>,
    )

    expect(screen.getByTestId('chat-input')).toHaveAttribute(
      'placeholder',
      'Ask anything...',
    )
  })
})
