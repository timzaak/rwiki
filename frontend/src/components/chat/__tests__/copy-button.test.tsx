import { render, screen, fireEvent, act } from '@testing-library/react'

import { CopyButton } from '@/components/chat/copy-button'

// jsdom has no Clipboard API; tests install their own navigator.clipboard.
function stubClipboard(writeText: ReturnType<typeof vi.fn>) {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    configurable: true,
  })
}

function restoreClipboard() {
  // @ts-expect-error test-only: delete the stubbed property
  delete navigator.clipboard
}

describe('CopyButton', () => {
  afterEach(() => {
    vi.useRealTimers()
    restoreClipboard()
  })

  it('copies the given text via the Clipboard API and shows copied feedback', async () => {
    // WHY: the whole point of the button is that message content reaches the
    // user's clipboard — asserting the writeText payload, not just a click.
    const writeText = vi.fn().mockResolvedValue(undefined)
    stubClipboard(writeText)

    render(<CopyButton text="AI response" />)
    const button = screen.getByTestId('message-copy-button')
    expect(button).toHaveAttribute('aria-label', 'Copy message')

    await act(async () => {
      fireEvent.click(button)
    })

    expect(writeText).toHaveBeenCalledWith('AI response')
    expect(button).toHaveAttribute('aria-label', 'Copied')
  })

  it('reverts the copied feedback after the timeout', async () => {
    vi.useFakeTimers()
    const writeText = vi.fn().mockResolvedValue(undefined)
    stubClipboard(writeText)

    render(<CopyButton text="AI response" />)
    const button = screen.getByTestId('message-copy-button')
    await act(async () => {
      fireEvent.click(button)
    })

    act(() => {
      vi.advanceTimersByTime(2000)
    })

    expect(button).toHaveAttribute('aria-label', 'Copy message')
  })

  it('falls back to execCommand when the Clipboard API is unavailable', async () => {
    // WHY: the widget can be embedded into non-secure (http) host pages where
    // navigator.clipboard is undefined — copying must still work there.
    restoreClipboard()
    const execCommand = vi.fn(() => {
      // The temp textarea is still in the DOM when execCommand runs.
      const textarea = document.querySelector('textarea')
      return Boolean(textarea && textarea.value === 'AI response')
    })
    document.execCommand = execCommand

    render(<CopyButton text="AI response" />)

    await act(async () => {
      fireEvent.click(screen.getByTestId('message-copy-button'))
    })

    expect(execCommand).toHaveBeenCalledWith('copy')
    expect(screen.getByTestId('message-copy-button')).toHaveAttribute(
      'aria-label',
      'Copied',
    )
    // The temporary textarea must not linger in the DOM.
    expect(document.querySelector('textarea')).not.toBeInTheDocument()
  })

  it('shows no copied feedback when every copy path fails', async () => {
    restoreClipboard()
    document.execCommand = vi.fn(() => false)

    render(<CopyButton text="AI response" />)

    await act(async () => {
      fireEvent.click(screen.getByTestId('message-copy-button'))
    })

    expect(screen.getByTestId('message-copy-button')).toHaveAttribute(
      'aria-label',
      'Copy message',
    )
  })
})
