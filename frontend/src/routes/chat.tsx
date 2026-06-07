import { useState, useEffect } from 'react'
import { createFileRoute } from '@tanstack/react-router'

import { ChatPanel } from '@/components/chat/chat-panel'
import { suggestions } from '@/lib/api-generated/sdk.gen'

export const Route = createFileRoute('/chat')({
  component: ChatPage,
})

const suggestionCache = new Map<string, string[]>()

function ChatPage() {
  const [suggestedQuestions, setSuggestedQuestions] = useState<string[]>(() => {
    const cached = suggestionCache.get(navigator.language)
    return cached ?? []
  })

  useEffect(() => {
    const locale = navigator.language
    if (suggestionCache.has(locale)) return

    let cancelled = false
    suggestions({ query: { locale } })
      .then((result) => {
        if (!cancelled && result.data) {
          suggestionCache.set(locale, result.data.questions)
          setSuggestedQuestions(result.data.questions)
        }
      })
      .catch(() => {
        // API failure degrades to empty array
      })
    return () => { cancelled = true }
  }, [])

  return (
    <div className="flex h-screen flex-col bg-background">
      <header className="flex items-center gap-3 border-b border-border/60 px-5 py-3">
        <div className="flex size-8 items-center justify-center rounded-lg bg-primary">
          <span className="font-serif text-sm font-bold text-primary-foreground">R</span>
        </div>
        <div>
          <h1 className="font-serif text-sm font-semibold tracking-tight">Rwiki Chat</h1>
          <p className="text-[11px] text-muted-foreground">Ask anything about your knowledge base</p>
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          <span className="inline-block size-1.5 rounded-full bg-emerald-500" />
          <span className="text-[11px] text-muted-foreground">Online</span>
        </div>
      </header>

      <div className="min-h-0 flex-1">
        <ChatPanel showHeader={false} suggestedQuestions={suggestedQuestions} />
      </div>
    </div>
  )
}
