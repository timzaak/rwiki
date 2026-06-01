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
        className="flex flex-1 flex-col items-center justify-center gap-3 text-muted-foreground"
      >
        <MessageSquareIcon className="size-10 opacity-40" />
        <p className="text-sm">Anything you'd like to know? Just ask.</p>
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
