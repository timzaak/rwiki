import { createContext, useContext } from 'react'

import { useChatStream } from '@/hooks/use-chat-stream'

interface ChatStreamValue {
  sendMessage: (content: string) => Promise<void>
  stopStreaming: () => void
}

const ChatStreamContext = createContext<ChatStreamValue | null>(null)

/**
 * Provider that uses the default SDK-based useChatStream hook.
 * Used by the admin app.
 */
export function DefaultChatStreamProvider({ children }: { children: React.ReactNode }) {
  const value = useChatStream()
  return (
    <ChatStreamContext.Provider value={value}>
      {children}
    </ChatStreamContext.Provider>
  )
}

/**
 * Generic provider that accepteds any ChatStreamValue.
 * Used by the widget with its independent SSE hook.
 */
export function ChatStreamProvider({ children, value }: {
  children: React.ReactNode
  value: ChatStreamValue
}) {
  return (
    <ChatStreamContext.Provider value={value}>
      {children}
    </ChatStreamContext.Provider>
  )
}

/**
 * Hook that returns the chat stream functions from context.
 * ChatInput should use this instead of directly calling useChatStream().
 */
export function useChatStreamContext(): ChatStreamValue {
  const value = useContext(ChatStreamContext)
  if (!value) {
    throw new Error(
      'useChatStreamContext must be used within a ChatStreamProvider or DefaultChatStreamProvider'
    )
  }
  return value
}

export type { ChatStreamValue }
