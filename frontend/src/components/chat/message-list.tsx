import { useEffect, useRef } from 'react'
import { MessageSquareIcon } from 'lucide-react'

import { useChatStore } from '@/stores/chat-store'
import { MessageItem } from './message-item'

interface MessageListProps {
  onRetry?: () => void
}

export function MessageList({ onRetry }: MessageListProps) {
  const messages = useChatStore((s) => s.messages)
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  if (messages.length === 0) {
    return (
      <div
        data-testid="message-list-empty"
        className="flex flex-1 flex-col items-center justify-center px-6 text-muted-foreground"
      >
        <div className="flex size-10 items-center justify-center rounded-xl bg-secondary/60 ring-1 ring-border/40">
          <MessageSquareIcon className="size-4.5 opacity-40" />
        </div>
        <p className="mt-2.5 font-serif text-xs font-medium opacity-50">Ask me anything</p>
        <p className="mt-1 text-[10px] opacity-40">Your knowledge base, conversationally.</p>
      </div>
    )
  }

  return (
    <div data-testid="message-list" className="flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {messages.map((message) => (
        <MessageItem key={message.id} message={message} onRetry={onRetry} />
      ))}
      <div ref={bottomRef} />
    </div>
  )
}
