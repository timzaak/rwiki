import { useEffect, useRef } from 'react'
import { MessageSquareIcon } from 'lucide-react'

import { useChatStore } from '@/stores/chat-store'
import { MessageItem } from './message-item'

export function MessageList() {
  const messages = useChatStore((s) => s.messages)
  const bottomRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  if (messages.length === 0) {
    return (
      <div
        data-testid="message-list-empty"
        className="flex flex-1 flex-col items-center justify-center gap-4 px-6 text-muted-foreground"
      >
        <div className="flex size-16 items-center justify-center rounded-2xl bg-secondary/60 ring-1 ring-border/40">
          <MessageSquareIcon className="size-7 opacity-50" />
        </div>
        <div className="text-center">
          <p className="font-serif text-sm font-medium">Ask me anything</p>
          <p className="mt-1 text-xs opacity-60">Your knowledge base, conversationally.</p>
        </div>
      </div>
    )
  }

  return (
    <div data-testid="message-list" className="flex-1 overflow-y-auto">
      {messages.map((message) => (
        <MessageItem key={message.id} message={message} />
      ))}
      <div ref={bottomRef} />
    </div>
  )
}
