import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { useChatStreamContext } from '@/components/chat/chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import { ChatInput } from '@/components/chat/chat-input'

vi.mock('@/components/chat/chat-stream-context', () => ({
  useChatStreamContext: vi.fn(),
}))

const mockSendMessage = vi.fn()
const mockStopStreaming = vi.fn()

function setup() {
  vi.mocked(useChatStreamContext).mockReturnValue({
    sendMessage: mockSendMessage,
    stopStreaming: mockStopStreaming,
  } as never)
  return {
    user: userEvent.setup(),
  }
}

/**
 * Inserts whitespace content into the textarea using the most appropriate
 * method for each character type, since userEvent.type cannot handle
 * empty strings and interprets \t as Tab (focus move).
 */
async function insertWhitespace(
  user: ReturnType<typeof userEvent.setup>,
  textarea: HTMLElement,
  label: string,
) {
  await user.click(textarea)
  switch (label) {
    case 'empty string':
      // nothing to type
      break
    case 'spaces only':
      await user.type(textarea, '   ')
      break
    case 'tabs only':
      fireEvent.change(textarea, { target: { value: '\t\t' } })
      break
    case 'newlines only':
      await user.keyboard('{Shift>}{Enter}{/Shift}')
      await user.keyboard('{Shift>}{Enter}{/Shift}')
      break
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  useChatStore.setState({
    messages: [],
    sessionId: null,
    isLoading: false,
    error: null,
  })
})

describe('ChatInput keyboard handling', () => {
  it('Enter key sends the message and clears input', async () => {
    const { user } = setup()
    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'Hello world')
    await user.keyboard('{Enter}')

    expect(mockSendMessage).toHaveBeenCalledWith('Hello world')
    expect(textarea).toHaveValue('')
  })

  it('Shift+Enter inserts a newline and does not send', async () => {
    const { user } = setup()
    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'Hello')
    await user.keyboard('{Shift>}{Enter}{/Shift}')
    await user.type(textarea, 'world')

    expect(mockSendMessage).not.toHaveBeenCalled()
    expect((textarea as HTMLTextAreaElement).value).toContain('\n')
  })

  it.each([
    ['empty string'],
    ['spaces only'],
    ['tabs only'],
    ['newlines only'],
  ])('does not send empty or whitespace-only messages (%s)', async (label) => {
    const { user } = setup()
    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await insertWhitespace(user, textarea, label)
    await user.keyboard('{Enter}')

    expect(mockSendMessage).not.toHaveBeenCalled()
  })

  it.each([
    ['isComposing (standard browsers)', { isComposing: true }],
    ['keyCode 229 (Safari composition end)', { keyCode: 229 }],
  ])(
    'does not send when Enter confirms an IME composition (%s)',
    (_label, keyInit) => {
      // WHY: 中文 IME 里按 Enter 是确认候选词而不是提交草稿；无守卫会把
      // 组合中的文本当作消息发送并清空输入框。
      setup()
      render(<ChatInput />)

      const textarea = screen.getByTestId('chat-input')
      fireEvent.change(textarea, { target: { value: '正在组合的中文' } })
      fireEvent.keyDown(textarea, { key: 'Enter', ...keyInit })

      expect(mockSendMessage).not.toHaveBeenCalled()
      expect(textarea).toHaveValue('正在组合的中文')
    },
  )

  it('send button click sends the message', async () => {
    const { user } = setup()
    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'test message')
    await user.click(screen.getByTestId('chat-send-button'))

    expect(mockSendMessage).toHaveBeenCalledWith('test message')
  })

  it('send button is disabled when input is empty', async () => {
    const { user } = setup()
    render(<ChatInput />)

    const sendButton = screen.getByTestId('chat-send-button')
    expect(sendButton).toBeDisabled()

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'text')
    expect(sendButton).toBeEnabled()

    await user.clear(textarea)
    expect(sendButton).toBeDisabled()
  })

  it('textarea stays enabled while streaming; the button morphs into the stop button', () => {
    // WHY: 流式期间输入区常可用是隐式打断的入口；按钮以 isLoading 为
    // 唯一事实来源变形，避免"点停止误发送"。
    useChatStore.setState({ isLoading: true })
    setup()

    render(<ChatInput />)

    expect(screen.getByTestId('chat-input')).toBeEnabled()
    expect(screen.getByTestId('chat-stop-button')).toBeInTheDocument()
    expect(screen.queryByTestId('chat-send-button')).not.toBeInTheDocument()
  })
})

describe('ChatInput stop button', () => {
  it('is exposed under an accessible name while streaming', () => {
    useChatStore.setState({ isLoading: true })
    setup()

    render(<ChatInput />)

    expect(
      screen.getByRole('button', { name: 'Stop generating' }),
    ).toBeInTheDocument()
  })

  it('stays a stop button even with a non-empty draft and clicking it stops without sending or clearing the draft', async () => {
    // WHY: 停止只终止本轮生成——不发送、不清草稿；草稿非空时 Enter 才是
    // 发送入口，按钮不能因为草稿非空变回发送。
    useChatStore.setState({ isLoading: true })
    const { user } = setup()

    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'draft question')

    expect(screen.getByTestId('chat-stop-button')).toBeInTheDocument()
    await user.click(screen.getByTestId('chat-stop-button'))

    expect(mockStopStreaming).toHaveBeenCalledTimes(1)
    expect(mockSendMessage).not.toHaveBeenCalled()
    expect(textarea).toHaveValue('draft question')
  })

  it('Enter during streaming sends the new question and clears the draft (implicit interrupt entry)', async () => {
    useChatStore.setState({ isLoading: true })
    const { user } = setup()

    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    await user.type(textarea, 'new question')
    await user.keyboard('{Enter}')

    expect(mockSendMessage).toHaveBeenCalledWith('new question')
    expect(textarea).toHaveValue('')
    // The hook's sendMessage aborts the old stream itself; the input must
    // not route Enter through the stop path.
    expect(mockStopStreaming).not.toHaveBeenCalled()
  })

  it('Enter confirming an IME composition during streaming neither sends nor stops the answer', () => {
    // WHY: 流式期间 textarea 常可用（隐式打断入口）后，IME 组合确认键若被
    // 当作发送，组合中文本会作为新问题发出并打断正在生成的回答——确认
    // 候选词不是发送意图。
    useChatStore.setState({ isLoading: true })
    setup()

    render(<ChatInput />)

    const textarea = screen.getByTestId('chat-input')
    fireEvent.change(textarea, { target: { value: '下一个问题' } })
    fireEvent.keyDown(textarea, { key: 'Enter', isComposing: true })

    expect(mockSendMessage).not.toHaveBeenCalled()
    expect(mockStopStreaming).not.toHaveBeenCalled()
    expect(textarea).toHaveValue('下一个问题')
  })
})
