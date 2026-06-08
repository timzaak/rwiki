import { ThumbsDown, ThumbsUp } from 'lucide-react'

import { cn } from '@/lib/utils'

interface FeedbackButtonsProps {
  feedback: 'like' | 'dislike' | undefined
  onFeedback: (type: 'like' | 'dislike') => void
  isSubmitting?: boolean
}

export function FeedbackButtons({
  feedback,
  onFeedback,
  isSubmitting,
}: FeedbackButtonsProps) {
  return (
    <div className="flex items-center gap-1 mt-1.5 justify-end">
      <button
        type="button"
        data-testid="feedback-like-button"
        className={cn(
          'p-1 rounded-sm transition-colors',
          feedback === 'like'
            ? 'text-primary'
            : 'text-muted-foreground/60 hover:text-foreground',
        )}
        onClick={() => onFeedback('like')}
        disabled={isSubmitting}
        aria-label="Like"
        aria-pressed={feedback === 'like'}
      >
        <ThumbsUp className="size-3.5" />
      </button>
      <button
        type="button"
        data-testid="feedback-dislike-button"
        className={cn(
          'p-1 rounded-sm transition-colors',
          feedback === 'dislike'
            ? 'text-primary'
            : 'text-muted-foreground/60 hover:text-foreground',
        )}
        onClick={() => onFeedback('dislike')}
        disabled={isSubmitting}
        aria-label="Dislike"
        aria-pressed={feedback === 'dislike'}
      >
        <ThumbsDown className="size-3.5" />
      </button>
    </div>
  )
}
