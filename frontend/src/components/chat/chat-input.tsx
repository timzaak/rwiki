import { useState, type KeyboardEvent } from 'react'
import { SendHorizonalIcon } from 'lucide-react'

import { useChatStreamContext } from './chat-stream-context'
import { useChatStore } from '@/stores/chat-store'
import { Button } from '@/components/ui/button'
import { useWidgetI18n } from './widget-i18n'

export function ChatInput() {
  const [input, setInput] = useState('')
  const { sendMessage } = useChatStreamContext()
  const isLoading = useChatStore((s) => s.isLoading)
  const t = useWidgetI18n()

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
    <div className="flex items-end gap-2 border-t border-border/60 px-4 py-3">
      <textarea
        data-testid="chat-input"
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={isLoading}
        placeholder={t.inputPlaceholder}
        rows={1}
        className="max-h-32 min-h-[2.25rem] flex-1 resize-none rounded-xl border border-border/60 bg-card px-3.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground/60 focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15 disabled:opacity-50 transition-colors"
      />
      <Button
        data-testid="chat-send-button"
        size="icon-sm"
        onClick={handleSend}
        disabled={!trimmed || isLoading}
        className="rounded-xl transition-all"
      >
        <SendHorizonalIcon className="size-4" />
      </Button>
    </div>
  )
}
