import { useCallback, useEffect, useRef } from 'react'

import { chat } from '@/lib/api-generated/sdk.gen'
import type { ChatRequest } from '@/lib/api-generated/types.gen'
import { detectEventType } from '@/lib/chat-sse'
import { useChatStore } from '@/stores/chat-store'
import { useChannelId } from '@/components/chat/channel-id-context'

export function useChatStream() {
  const abortRef = useRef<AbortController | null>(null)
  const channelId = useChannelId()

  const {
    sessionId,
    addUserMessage,
    addAssistantMessage,
    appendToLastAssistant,
    finishStreaming,
    markContentEnded,
    interruptStreaming,
    setLastAssistantSuggestions,
    setSessionId,
    setError,
  } = useChatStore()

  const stopStreaming = useCallback(() => {
    abortRef.current?.abort()
  }, [])

  const sendMessage = useCallback(
    async (content: string) => {
      abortRef.current?.abort()

      const controller = new AbortController()
      abortRef.current = controller

      addUserMessage(content)
      const assistantId = crypto.randomUUID()
      addAssistantMessage(assistantId)

      try {
        const body: ChatRequest = {
          message: content,
          sessionId: sessionId,
          channelId,
          supportsContentEndEvent: true,
        }

        const result = await chat({
          body,
          signal: controller.signal,
          sseMaxRetryAttempts: 0,
        })

        for await (const event of result.stream) {
          // Re-check abort in case stopStreaming was called during iteration
          if (controller.signal.aborted) break

          const eventType = detectEventType(event)

          switch (eventType) {
            case 'session':
              setSessionId(
                (event as Record<string, unknown>).sessionId as string,
              )
              break
            case 'chunk':
              appendToLastAssistant(
                String((event as Record<string, unknown>).content),
              )
              break
            case 'suggestions':
              setLastAssistantSuggestions(
                (event as Record<string, unknown>).suggestions as string[],
              )
              break
            case 'contentEnd':
              markContentEnded(assistantId)
              break
            case 'error':
              setError(
                String((event as Record<string, unknown>).message),
              )
              finishStreaming()
              return
            case 'done':
              finishStreaming()
              return
          }
        }

        // Stream ended: aborted means user interrupt, otherwise normal completion
        if (controller.signal.aborted) {
          interruptStreaming(assistantId)
        } else {
          finishStreaming()
        }
      } catch (err: unknown) {
        // AbortError means user cancelled — finish this round as interrupted,
        // preserve displayed content, no error
        if (controller.signal.aborted) {
          interruptStreaming(assistantId)
          return
        }

        // Network or other error — preserve displayed content, set error message
        let message =
          err instanceof Error ? err.message : 'Connection lost. Please try again.'

        if (message.startsWith('SSE failed:')) {
          if (message.includes('503')) {
            message = 'No indexed data in knowledge base. Please upload a document first.'
          } else if (message.includes('400')) {
            message = 'Invalid request. Please check your input.'
          }
        }

        setError(message)
        finishStreaming()
      }
    },
    [
      sessionId,
      channelId,
      addUserMessage,
      addAssistantMessage,
      appendToLastAssistant,
      finishStreaming,
      markContentEnded,
      interruptStreaming,
      setLastAssistantSuggestions,
      setSessionId,
      setError,
    ],
  )

  useEffect(() => {
    return () => {
      abortRef.current?.abort()
    }
  }, [])

  return { sendMessage, stopStreaming }
}
