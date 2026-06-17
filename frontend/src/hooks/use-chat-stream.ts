import { useCallback, useEffect, useRef } from 'react'

import { chat } from '@/lib/api-generated/sdk.gen'
import type { ChatRequest } from '@/lib/api-generated/types.gen'
import { useChatStore } from '@/stores/chat-store'

function detectEventType(
  data: unknown,
): 'session' | 'chunk' | 'suggestions' | 'error' | 'done' {
  if (typeof data !== 'object' || data === null) return 'done'
  const record = data as Record<string, unknown>
  if ('sessionId' in record && record.sessionId) return 'session'
  if ('content' in record && record.content !== undefined) return 'chunk'
  if ('suggestions' in record && Array.isArray(record.suggestions))
    return 'suggestions'
  if ('message' in record && record.message) return 'error'
  return 'done'
}

export function useChatStream() {
  const abortRef = useRef<AbortController | null>(null)

  const {
    sessionId,
    addUserMessage,
    addAssistantMessage,
    appendToLastAssistant,
    finishStreaming,
    setLastAssistantSuggestions,
    setSessionId,
    setError,
  } = useChatStore()

  const stopStreaming = useCallback(() => {
    abortRef.current?.abort()
  }, [])

  const sendMessage = useCallback(
    async (content: string) => {
      // Abort any in-progress stream
      abortRef.current?.abort()

      const controller = new AbortController()
      abortRef.current = controller

      // Add user message and placeholder assistant message
      addUserMessage(content)
      const assistantId = crypto.randomUUID()
      addAssistantMessage(assistantId)

      try {
        const body: ChatRequest = {
          message: content,
          sessionId: sessionId,
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

        // Stream ended normally
        finishStreaming()
      } catch (err: unknown) {
        // AbortError means user cancelled — preserve displayed content, no error
        if (controller.signal.aborted) {
          finishStreaming()
          return
        }

        // Network or other error — preserve displayed content, set error message
        let message =
          err instanceof Error ? err.message : 'Connection lost. Please try again.'

        // Map SSE HTTP errors to user-friendly messages
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
      addUserMessage,
      addAssistantMessage,
      appendToLastAssistant,
      finishStreaming,
      setLastAssistantSuggestions,
      setSessionId,
      setError,
    ],
  )

  // Cleanup: abort on unmount
  useEffect(() => {
    return () => {
      abortRef.current?.abort()
    }
  }, [])

  return { sendMessage, stopStreaming }
}
