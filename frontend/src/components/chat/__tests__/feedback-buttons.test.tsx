import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import { FeedbackButtons } from '@/components/chat/feedback-buttons'

function renderFeedbackButtons(
  overrides?: Partial<React.ComponentProps<typeof FeedbackButtons>>,
) {
  const defaults: React.ComponentProps<typeof FeedbackButtons> = {
    feedback: undefined,
    onFeedback: vi.fn(),
    isSubmitting: false,
  }
  const props = { ...defaults, ...overrides }
  const user = userEvent.setup()
  return {
    user,
    onFeedback: props.onFeedback,
    ...render(<FeedbackButtons {...props} />),
  }
}

describe('FeedbackButtons rendering', () => {
  it('renders both like and dislike buttons', () => {
    renderFeedbackButtons()

    expect(screen.getByTestId('feedback-like-button')).toBeInTheDocument()
    expect(screen.getByTestId('feedback-dislike-button')).toBeInTheDocument()
  })

  it('highlights like button when feedback is like', () => {
    renderFeedbackButtons({ feedback: 'like' })

    expect(screen.getByTestId('feedback-like-button')).toHaveAttribute(
      'aria-pressed',
      'true',
    )
    expect(screen.getByTestId('feedback-dislike-button')).toHaveAttribute(
      'aria-pressed',
      'false',
    )
  })

  it('highlights dislike button when feedback is dislike', () => {
    renderFeedbackButtons({ feedback: 'dislike' })

    expect(screen.getByTestId('feedback-dislike-button')).toHaveAttribute(
      'aria-pressed',
      'true',
    )
    expect(screen.getByTestId('feedback-like-button')).toHaveAttribute(
      'aria-pressed',
      'false',
    )
  })
})

describe('FeedbackButtons interaction', () => {
  it('calls onFeedback with like when like button clicked', async () => {
    const onFeedback = vi.fn()
    const { user } = renderFeedbackButtons({ onFeedback })

    await user.click(screen.getByTestId('feedback-like-button'))

    expect(onFeedback).toHaveBeenCalledWith('like')
  })

  it('calls onFeedback with dislike when dislike button clicked', async () => {
    const onFeedback = vi.fn()
    const { user } = renderFeedbackButtons({ onFeedback })

    await user.click(screen.getByTestId('feedback-dislike-button'))

    expect(onFeedback).toHaveBeenCalledWith('dislike')
  })
})

describe('FeedbackButtons disabled state', () => {
  it('disables both buttons when isSubmitting is true', () => {
    renderFeedbackButtons({ isSubmitting: true })

    expect(screen.getByTestId('feedback-like-button')).toBeDisabled()
    expect(screen.getByTestId('feedback-dislike-button')).toBeDisabled()
  })

  it('enables both buttons when isSubmitting is false', () => {
    renderFeedbackButtons({ isSubmitting: false })

    expect(screen.getByTestId('feedback-like-button')).toBeEnabled()
    expect(screen.getByTestId('feedback-dislike-button')).toBeEnabled()
  })
})
