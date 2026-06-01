import { useState, type KeyboardEvent } from 'react'
import { SendHorizonalIcon } from 'lucide-react'

import { useChatStreamContext } from './chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import { Button } from '@/components/ui/button'

export function ChatInput() {
  const [input, setInput] = useState('')
  const { sendMessage } = useChatStreamContext()
  const isLoading = useChatStore((s) => s.isLoading)

  const trimmed = input.trim()

  function handleSend() {
    if (!trimmed || isLoading) return
    sendMessage(trimmed)
    setInput('')
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      handleSend()
    }
  }

  return (
    <div className="flex items-end gap-2 border-t px-4 py-3">
      <textarea
        data-testid="chat-input"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={isLoading}
        placeholder="Type your message..."
        rows={1}
        className="max-h-32 min-h-[2.25rem] flex-1 resize-none rounded-lg border border-input bg-background px-3 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/50 disabled:opacity-50"
      />
      <Button
        data-testid="chat-send-button"
        size="icon-sm"
        onClick={handleSend}
        disabled={!trimmed || isLoading}
      >
        <SendHorizonalIcon className="size-4" />
      </Button>
    </div>
  )
}
