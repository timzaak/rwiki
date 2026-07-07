import { useCallback, useRef, useState } from 'react'

import { submitFeedback } from '@/lib/api-generated/sdk.gen'
import type { FeedbackRequest } from '@/lib/api-generated/types.gen'
import { useChatStore } from '@/stores/chat-store'
import { useSiteId } from '@/components/chat/site-id-context'

import { useFeedbackSubmitFnFromContext } from './feedback-context'

export interface UseFeedbackOptions {
  sessionId: string | null
  messageId: string
  userMessage: string
  assistantMessage: string
}

export interface UseFeedbackReturn {
  feedback: 'like' | 'dislike' | undefined
  submitFeedback: (type: 'like' | 'dislike') => Promise<void>
  isSubmitting: boolean
}

export function useFeedback(options: UseFeedbackOptions): UseFeedbackReturn {
  const { sessionId, messageId, userMessage, assistantMessage } = options
  const siteId = useSiteId()

  const feedback = useChatStore(
    (s) =>
      s.messages.find((m) => m.id === messageId)?.feedback as
        | 'like'
        | 'dislike'
        | undefined,
  )
  const updateMessageFeedback = useChatStore((s) => s.updateMessageFeedback)
  const contextSubmitFn = useFeedbackSubmitFnFromContext()

  const [isSubmitting, setIsSubmitting] = useState(false)
  const previousFeedbackRef = useRef(feedback)
  const submittingRef = useRef(false)

  const handleSubmitFeedback = useCallback(
    async (type: 'like' | 'dislike') => {
      if (sessionId === null || submittingRef.current) return

      // Determine target: toggle off if clicking same, otherwise set new value
      const target = feedback === type ? undefined : type

      // Save current for rollback
      previousFeedbackRef.current = feedback

      // Optimistic update
      updateMessageFeedback(messageId, target)
      submittingRef.current = true
      setIsSubmitting(true)

      const body: FeedbackRequest = {
        sessionId,
        messageId,
        feedback: target ?? null,
        userMessage,
        assistantMessage,
        siteId,
      }

      try {
        if (contextSubmitFn) {
          await contextSubmitFn(body)
        } else {
          await submitFeedback<true>({ body, throwOnError: true })
        }
      } catch (err: unknown) {
        // Rollback on error
        updateMessageFeedback(messageId, previousFeedbackRef.current)
        console.error('Failed to submit feedback:', err)
      } finally {
        submittingRef.current = false
        setIsSubmitting(false)
      }
    },
    [
      sessionId,
      siteId,
      messageId,
      feedback,
      userMessage,
      assistantMessage,
      updateMessageFeedback,
      contextSubmitFn,
    ],
  )

  return { feedback, submitFeedback: handleSubmitFeedback, isSubmitting }
}
