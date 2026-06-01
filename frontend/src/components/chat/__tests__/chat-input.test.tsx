import { fireEvent, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { useChatStreamContext } from '@/components/chat/chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import { ChatInput } from '@/components/chat/chat-input'

vi.mock('@/components/chat/chat-stream-context', () => ({
  useChatStreamContext: vi.fn(),
}))

const mockSendMessage = vi.fn()

function setup() {
  vi.mocked(useChatStreamContext).mockReturnValue({
    sendMessage: mockSendMessage,
    stopStreaming: vi.fn(),
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

  it('textarea is disabled when isLoading is true', () => {
    useChatStore.setState({ isLoading: true })
    setup()

    render(<ChatInput />)

    expect(screen.getByTestId('chat-input')).toBeDisabled()
    expect(screen.getByTestId('chat-send-button')).toBeDisabled()
  })
})
