import { HistoryIcon, PlusIcon } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'

import { cn } from '@/lib/utils'
import { useChatStore } from '@/stores/chat-store'
import {
  isHistoryStorageAvailable,
  startNewConversation,
} from '@/stores/chat-history-store'
import { useChatStreamContext } from './chat-stream-context'
import { Button } from '@/components/ui/button'
import { MessageList } from './message-list'
import { ChatInput } from './chat-input'
import { SuggestedQuestions } from './suggested-questions'
import { ChatHistoryPanel } from './chat-history-panel'
import { useWidgetI18n } from './widget-i18n'

interface ChatPanelProps {
  className?: string
  showHeader?: boolean
  headerTitle?: string
  welcomeMessage?: string
  suggestedQuestions?: string[]
}

export function ChatPanel({
  className,
  showHeader = false,
  headerTitle = 'Chat Assistant',
  welcomeMessage,
  suggestedQuestions,
}: ChatPanelProps) {
  const messages = useChatStore((s) => s.messages)
  const sessionId = useChatStore((s) => s.sessionId)
  const isLoading = useChatStore((s) => s.isLoading)
  const error = useChatStore((s) => s.error)
  const setError = useChatStore((s) => s.setError)
  const { sendMessage, stopStreaming } = useChatStreamContext()
  const t = useWidgetI18n()
  const historyAvailable = isHistoryStorageAvailable()
  const [historyOpen, setHistoryOpen] = useState(false)
  const messageCountRef = useRef(messages.length)
  const hasConversation = messages.length > 0 || Boolean(sessionId) || Boolean(error)
  // 稳定引用：历史面板的 Escape 监听 effect 依赖 onClose，流式 chunk 引发的
  // 重渲染不应反复重建 window 监听。
  const closeHistory = useCallback(() => setHistoryOpen(false), [])

  // 新消息加入（用户提问或回答占位）即回到对话视图；流式 chunk 不改变条数，
  // 不会把列表收起，流式中打开的历史列表保持可用。
  useEffect(() => {
    if (messages.length > messageCountRef.current) setHistoryOpen(false)
    messageCountRef.current = messages.length
  }, [messages.length])

  function handleNewConversation() {
    if (isLoading) stopStreaming()
    startNewConversation()
  }

  function handleRetry() {
    setError('')
    const { messages, removeLastFailedPair } = useChatStore.getState()
    const lastUserMsg = [...messages]
      .reverse()
      .find((m) => m.role === 'user')
    if (lastUserMsg) {
      removeLastFailedPair()
      sendMessage(lastUserMsg.content)
    }
  }

  const toolButtons = (
    <div className="flex items-center gap-1">
      {historyAvailable && (
        <Button
          data-testid="chat-history-toggle-button"
          size="icon-xs"
          variant="ghost"
          onClick={() => setHistoryOpen((open) => !open)}
          aria-label={t.a11yHistory}
          aria-expanded={historyOpen}
        >
          <HistoryIcon className="size-3.5" />
        </Button>
      )}
      {hasConversation && (
        <Button
          data-testid="chat-new-conversation-button"
          size="icon-xs"
          variant="ghost"
          onClick={handleNewConversation}
          aria-label={t.a11yNewConversation}
        >
          <PlusIcon className="size-3.5" />
        </Button>
      )}
    </div>
  )

  return (
    <div
      data-testid="chat-panel"
      className={cn('flex h-full flex-col', className)}
    >
      {showHeader && (
        <div className="flex items-center justify-between gap-2 border-b border-border/60 px-4 py-2">
          <span className="font-serif text-sm font-medium">{headerTitle ?? t.titleDefault}</span>
          {toolButtons}
        </div>
      )}

      {!showHeader && (historyAvailable || hasConversation) && (
        <div className="flex items-center justify-end border-b border-border/60 px-4 py-2">
          {toolButtons}
        </div>
      )}

      {welcomeMessage && messages.length === 0 && (
        <div className="px-4 pt-3 text-xs text-muted-foreground/70">
          {welcomeMessage}
        </div>
      )}

      <div className="relative flex min-h-0 flex-1 flex-col">
        <MessageList onRetry={handleRetry} />
        {historyOpen && historyAvailable && (
          <ChatHistoryPanel onClose={closeHistory} />
        )}
      </div>

      {messages.length === 0 && suggestedQuestions && suggestedQuestions.length > 0 && (
        <div className="px-4" data-testid="chat-empty-suggestions">
          <SuggestedQuestions
            questions={suggestedQuestions}
            onSelect={sendMessage}
          />
        </div>
      )}

      <ChatInput />
    </div>
  )
}
