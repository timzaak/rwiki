import { useCallback, useEffect, useRef } from 'react'

import type { ChatStreamValue } from '@/components/chat/chat-stream-context'
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

function processSseLines(
  lines: string[],
  store: ReturnType<typeof useChatStore.getState>,
): boolean {
  for (const line of lines) {
    if (line.startsWith('event: ')) continue
    if (!line.startsWith('data: ')) continue
    const data = line.slice(6)

    try {
      const parsed = JSON.parse(data)
      switch (detectEventType(parsed)) {
        case 'session':
          store.setSessionId(parsed.sessionId as string)
          break
        case 'chunk':
          store.appendToLastAssistant(String(parsed.content))
          break
        case 'suggestions':
          store.setLastAssistantSuggestions(parsed.suggestions as string[])
          break
        case 'error':
          store.setError(String(parsed.message ?? 'Failed to generate response'))
          store.finishStreaming()
          return true
        case 'done':
          store.finishStreaming()
          return true
      }
    } catch {
      if (data.trim()) store.appendToLastAssistant(data)
    }
  }
  return false
}

export function useWidgetChatStream(apiUrl: string, channelId: string): ChatStreamValue {
  const abortRef = useRef<AbortController | null>(null)
  const storeRef = useRef(useChatStore.getState())
  storeRef.current = useChatStore.getState()

  const stopStreaming = useCallback(() => {
    abortRef.current?.abort()
  }, [])

  const sendMessage = useCallback(
    async (content: string) => {
      abortRef.current?.abort()
      const controller = new AbortController()
      abortRef.current = controller

      const store = storeRef.current
      store.addUserMessage(content)
      const assistantId = crypto.randomUUID()
      store.addAssistantMessage(assistantId)

      try {
        const response = await fetch(`${apiUrl}/api/chat`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            message: content,
            sessionId: store.sessionId,
            channelId,
          }),
          signal: controller.signal,
        })

        if (!response.ok) {
          store.setError(`Request failed (${response.status})`)
          store.finishStreaming()
          return
        }

        const reader = response.body!.getReader()
        const decoder = new TextDecoder()
        let buffer = ''

        while (true) {
          const { done, value } = await reader.read()
          if (done) {
            store.finishStreaming()
            break
          }

          buffer += decoder.decode(value, { stream: true })
          const lines = buffer.split('\n')
          buffer = lines.pop()!

          if (processSseLines(lines, store)) return
        }
      } catch {
        if (controller.signal.aborted) {
          store.finishStreaming()
          return
        }
        store.setError('Unable to connect to server. Please check your configuration or try again later.')
        store.finishStreaming()
      }
    },
    [apiUrl, channelId],
  )

  useEffect(() => {
    return () => {
      abortRef.current?.abort()
    }
  }, [])

  return { sendMessage, stopStreaming }
}
