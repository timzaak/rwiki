import { XIcon, RefreshCwIcon, Trash2Icon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { useChatStore } from '@/stores/chat-store'
import { useChatStreamContext } from './chat-stream-context'
import { Button } from '@/components/ui/button'
import { MessageList } from './message-list'
import { ChatInput } from './chat-input'

interface ChatPanelProps {
  className?: string
  showHeader?: boolean
  headerTitle?: string
  welcomeMessage?: string
}

export function ChatPanel({
  className,
  showHeader = false,
  headerTitle = 'Chat Assistant',
  welcomeMessage,
}: ChatPanelProps) {
  const messages = useChatStore((s) => s.messages)
  const sessionId = useChatStore((s) => s.sessionId)
  const isLoading = useChatStore((s) => s.isLoading)
  const error = useChatStore((s) => s.error)
  const setError = useChatStore((s) => s.setError)
  const clearMessages = useChatStore((s) => s.clearMessages)
  const { sendMessage, stopStreaming } = useChatStreamContext()
  const hasConversation = messages.length > 0 || Boolean(sessionId) || Boolean(error)

  function handleClearConversation() {
    if (isLoading) stopStreaming()
    clearMessages()
  }

  const clearButton = hasConversation ? (
    <Button
      data-testid="chat-clear-button"
      size="icon-xs"
      variant="ghost"
      onClick={handleClearConversation}
      aria-label="Clear current conversation"
    >
      <Trash2Icon className="size-3.5" />
    </Button>
  ) : null

  return (
    <div
      data-testid="chat-panel"
      className={cn('flex h-full flex-col', className)}
    >
      {showHeader && (
        <div className="flex items-center justify-between gap-2 border-b border-border/60 px-4 py-2">
          <span className="font-serif text-sm font-medium">{headerTitle}</span>
          {clearButton}
        </div>
      )}

      {!showHeader && hasConversation && (
        <div className="flex items-center justify-end border-b border-border/60 px-4 py-2">
          {clearButton}
        </div>
      )}

      {welcomeMessage && messages.length === 0 && (
        <div className="px-4 py-3 text-sm text-muted-foreground">
          {welcomeMessage}
        </div>
      )}

      <MessageList />

      {error && (
        <div
          data-testid="chat-error-banner"
          className="flex items-center justify-between gap-2 border-t border-destructive/20 bg-destructive/5 px-4 py-2 text-sm text-destructive"
        >
          <span className="flex-1 truncate">{error}</span>
          <div className="flex items-center gap-1">
            <Button
              size="icon-xs"
              variant="ghost"
              onClick={() => setError('')}
              aria-label="Dismiss error"
            >
              <XIcon className="size-3.5" />
            </Button>
            <Button
              size="icon-xs"
              variant="ghost"
              onClick={() => {
                setError('')
                const { messages, removeLastFailedPair } =
                  useChatStore.getState()
                const lastUserMsg = [...messages]
                  .reverse()
                  .find((m) => m.role === 'user')
                if (lastUserMsg) {
                  removeLastFailedPair()
                  sendMessage(lastUserMsg.content)
                }
              }}
              aria-label="Retry"
            >
              <RefreshCwIcon className="size-3.5" />
            </Button>
          </div>
        </div>
      )}

      <ChatInput />
    </div>
  )
}
