import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import type { ChatMessage } from '@/stores/chat-store'
import { useChatStore } from '@/stores/chat-store'
import { MessageItem } from '@/components/chat/message-item'
import { ChatStreamProvider } from '@/components/chat/chat-stream-context'
import type { ChatStreamValue } from '@/components/chat/chat-stream-context'
import { ChannelIdProvider } from '@/components/chat/channel-id-context'
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

// Default context value for the shared renderMessage helper. Individual tests
// that need to assert on sendMessage should call renderMessage directly with
// their own value via the renderWithStream helper below.
const defaultStreamValue: ChatStreamValue = {
  sendMessage: vi.fn(),
  stopStreaming: vi.fn(),
}

function renderWithStream(
  message: ChatMessage,
  value: ChatStreamValue = defaultStreamValue,
  extra?: { onRetry?: () => void },
) {
  return render(
    <ChatStreamProvider value={value}>
      <ChannelIdProvider channelId={['test-channel']}>
        <MessageItem message={message} onRetry={extra?.onRetry} />
      </ChannelIdProvider>
    </ChatStreamProvider>,
  )
}

function renderMessage(overrides?: Partial<ChatMessage>) {
  return renderWithStream(makeMessage(overrides))
}

describe('MessageItem conditional rendering', () => {
  it('shows streaming cursor when message is streaming', () => {
    renderMessage({ role: 'assistant', content: 'Hello', isStreaming: true })

    expect(screen.getByTestId('message-item-streaming')).toBeInTheDocument()
  })

  it('does not show streaming cursor when message is not streaming', () => {
    renderMessage({ role: 'assistant', content: 'Hello', isStreaming: false })

    expect(screen.queryByTestId('message-item-streaming')).not.toBeInTheDocument()
  })

  it('shows error indicator when message has error', () => {
    renderMessage({ role: 'assistant', content: 'Hello', error: 'Stream interrupted' })

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

  it('hides feedback buttons when assistant message is empty (failed)', () => {
    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: '',
      isStreaming: false,
    })

    expect(screen.queryByTestId('feedback-like-button')).not.toBeInTheDocument()
    expect(screen.queryByTestId('feedback-dislike-button')).not.toBeInTheDocument()
  })

  it('shows retry button when assistant message is empty and onRetry provided', () => {
    const onRetry = vi.fn()
    renderWithStream(
      makeMessage({ role: 'assistant', content: '', isStreaming: false }),
      defaultStreamValue,
      { onRetry },
    )

    expect(screen.getByTestId('message-retry-button')).toBeInTheDocument()
  })

  it('shows failed message text when assistant message is empty', () => {
    renderMessage({ role: 'assistant', content: '', isStreaming: false })

    expect(screen.getByText('Response generation failed. Please try again.')).toBeInTheDocument()
  })
})

describe('MessageItem suggested questions', () => {
  it('renders suggested questions between content and feedback when assistant message is finished and non-empty', () => {
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'msg-user-1', role: 'user', content: 'User question' }),
        makeMessage({
          id: 'msg-asst-1',
          role: 'assistant',
          content: 'AI response',
          isStreaming: false,
          suggestedQuestions: ['What is RAG?', 'How does it work?'],
        }),
      ],
      sessionId: 'test-session',
      isLoading: false,
      error: null,
    })
    client.setConfig({ baseUrl: 'http://localhost:3000' })

    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'AI response',
      isStreaming: false,
      suggestedQuestions: ['What is RAG?', 'How does it work?'],
    })

    const assistant = screen.getByTestId('message-item-assistant')
    const chips = within(assistant).getAllByTestId('suggested-question-button')
    expect(chips).toHaveLength(2)
    expect(chips[0]).toHaveTextContent('What is RAG?')
    expect(chips[1]).toHaveTextContent('How does it work?')

    // Guard of feedback is independent; both render for a finished non-empty assistant message.
    expect(within(assistant).getByTestId('suggested-questions')).toBeInTheDocument()
  })

  it('does not render suggested questions while assistant message is streaming', () => {
    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'partial',
      isStreaming: true,
      suggestedQuestions: ['What is RAG?'],
    })

    expect(screen.queryByTestId('suggested-questions')).not.toBeInTheDocument()
  })

  it('does not render suggested questions when assistant message is failed (empty content)', () => {
    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: '',
      isStreaming: false,
      suggestedQuestions: ['What is RAG?'],
    })

    expect(screen.queryByTestId('suggested-questions')).not.toBeInTheDocument()
  })

  it.each<[string, string[] | undefined]>([
    ['undefined', undefined],
    ['empty', []],
  ])(
    'does not render suggested questions when suggestedQuestions is %s',
    (_label, suggestedQuestions) => {
      renderMessage({
        id: 'msg-asst-1',
        role: 'assistant',
        content: 'AI response',
        isStreaming: false,
        suggestedQuestions,
      })

      expect(screen.queryByTestId('suggested-questions')).not.toBeInTheDocument()
    },
  )

  it('clicking a suggested question chip calls sendMessage with the question text', async () => {
    const user = userEvent.setup()
    const sendMessage = vi.fn()
    renderWithStream(
      makeMessage({
        id: 'msg-asst-1',
        role: 'assistant',
        content: 'AI response',
        isStreaming: false,
        suggestedQuestions: ['What is RAG?', 'How does it work?'],
      }),
      { sendMessage, stopStreaming: vi.fn() },
    )

    const assistant = screen.getByTestId('message-item-assistant')
    const chips = within(assistant).getAllByTestId('suggested-question-button')
    await user.click(chips[1])

    expect(sendMessage).toHaveBeenCalledWith('How does it work?')
    expect(sendMessage).toHaveBeenCalledTimes(1)
  })
})

describe('MessageItem interrupted messages', () => {
  function seedStore(messages: ChatMessage[]) {
    useChatStore.setState({
      messages,
      sessionId: 'test-session',
      isLoading: false,
      error: null,
    })
  }

  const interruptedMessage = makeMessage({
    id: 'msg-asst-1',
    role: 'assistant',
    content: 'partial answ',
    isStreaming: false,
    interrupted: true,
  })

  it('shows the interrupted notice instead of passing the truncated answer off as complete', () => {
    seedStore([
      makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q' }),
      interruptedMessage,
    ])

    renderMessage({ ...interruptedMessage })

    expect(screen.getByTestId('message-interrupted-notice')).toBeInTheDocument()
    expect(
      screen.getByText(
        'The response was interrupted; the content above may be incomplete.',
      ),
    ).toBeInTheDocument()
  })

  it('shows no interrupted notice for a completed message', () => {
    seedStore([
      makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q' }),
      makeMessage({
        id: 'msg-asst-1',
        role: 'assistant',
        content: 'full answer',
        isStreaming: false,
      }),
    ])

    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'full answer',
      isStreaming: false,
    })

    expect(
      screen.queryByTestId('message-interrupted-notice'),
    ).not.toBeInTheDocument()
  })

  it('offers no retry on interrupted messages — interruption is not an error', () => {
    // WHY: 用户主动打断不提示重试（打断即终止本轮，可自然语言追问
    // "继续"）；重试入口只属于 isFailed 网络失败分支。
    const onRetry = vi.fn()
    seedStore([
      makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q' }),
      interruptedMessage,
    ])

    renderWithStream({ ...interruptedMessage }, defaultStreamValue, { onRetry })

    expect(screen.getByTestId('message-interrupted-notice')).toBeInTheDocument()
    expect(
      screen.queryByTestId('message-retry-button'),
    ).not.toBeInTheDocument()
  })

  it('keeps the notice on an interrupted message when a newer turn exists after it', () => {
    // WHY: 中断标识是内容完整性事实，不因后续轮次消失；该消息也不再提供
    // 任何重试入口（见上一用例）。
    seedStore([
      makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q1' }),
      interruptedMessage,
      makeMessage({ id: 'msg-user-2', role: 'user', content: 'Q2' }),
      makeMessage({
        id: 'msg-asst-2',
        role: 'assistant',
        content: 'newer answer',
        isStreaming: false,
      }),
    ])

    renderWithStream({ ...interruptedMessage }, defaultStreamValue, {
      onRetry: vi.fn(),
    })

    expect(screen.getByTestId('message-interrupted-notice')).toBeInTheDocument()
    expect(
      screen.queryByTestId('message-retry-button'),
    ).not.toBeInTheDocument()
  })

  it('shows no interrupted notice for a contentEnded message (complete answer)', () => {
    // WHY: 回答主体已完成的轮次内容是完整的，不得呈现"可能不完整"提示。
    seedStore([
      makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q' }),
      makeMessage({
        id: 'msg-asst-1',
        role: 'assistant',
        content: 'full answer',
        isStreaming: false,
        contentEnded: true,
      }),
    ])

    renderMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: 'full answer',
      isStreaming: false,
      contentEnded: true,
    })

    expect(
      screen.queryByTestId('message-interrupted-notice'),
    ).not.toBeInTheDocument()
  })
})

describe('MessageItem empty but explained answers', () => {
  function renderEmpty(overrides: Partial<ChatMessage>, onRetry?: () => void) {
    const message = makeMessage({
      id: 'msg-asst-1',
      role: 'assistant',
      content: '',
      isStreaming: false,
      ...overrides,
    })
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'msg-user-1', role: 'user', content: 'Q' }),
        message,
      ],
      sessionId: 'test-session',
      isLoading: false,
      error: null,
    })
    return renderWithStream({ ...message }, defaultStreamValue, { onRetry })
  }

  it('renders a completed-but-empty answer as empty, not as a failure', () => {
    // WHY: contentEnd 后零 chunk 说明服务端正常完成但答案为空；isFailed
    // 启发式若不看 contentEnded，会把它渲染成"生成失败"+重试，把正常完成
    // 误报为失败。
    renderEmpty({ contentEnded: true }, vi.fn())

    expect(
      screen.queryByText('Response generation failed. Please try again.'),
    ).not.toBeInTheDocument()
    expect(screen.queryByTestId('message-retry-button')).not.toBeInTheDocument()
    expect(screen.getByTestId('message-empty-response')).toHaveTextContent(
      'The model returned an empty response.',
    )
  })

  it('renders a refresh-restored empty interrupted round with the notice, not as failed', () => {
    // WHY: 首内容前被打断的轮次经持久化恢复得到 content='' +
    // interrupted=true；打断不是错误，不得进入失败态。
    renderEmpty({ interrupted: true }, vi.fn())

    expect(
      screen.queryByText('Response generation failed. Please try again.'),
    ).not.toBeInTheDocument()
    expect(screen.queryByTestId('message-retry-button')).not.toBeInTheDocument()
    expect(screen.getByTestId('message-interrupted-notice')).toBeInTheDocument()
  })
})
