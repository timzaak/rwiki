import { useEffect, useState } from 'react'
import { XIcon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { useChatModalStore } from '@/stores/chat-store'
import { ChatPanel } from '@/components/chat/chat-panel'
import { Button } from '@/components/ui/button'
import { useWidgetI18n } from './widget-i18n'

interface ChatSidePanelProps {
  title?: string
  welcomeMessage?: string
  suggestedQuestions?: string[]
}

// Side-panel display mode shell: full-height, docked to the viewport's right
// edge, slides in/out, not draggable, no overlay (the host content is pushed
// left by side-panel-host.ts instead of being dimmed). Shares isModalOpen with
// the modal shell, so ask()/floating button route here unchanged.
export function ChatSidePanel({ title, welcomeMessage, suggestedQuestions }: ChatSidePanelProps) {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const closeModal = useChatModalStore((s) => s.closeModal)
  const t = useWidgetI18n()

  // closed → open (slide in) → closing (slide out) → closed; reopening during
  // the closing window cancels the unmount and slides back in.
  const [phase, setPhase] = useState<'closed' | 'open' | 'closing'>('closed')

  useEffect(() => {
    if (isModalOpen) {
      setPhase('open')
    } else {
      setPhase((p) => (p === 'open' ? 'closing' : 'closed'))
    }
  }, [isModalOpen])

  useEffect(() => {
    if (phase !== 'closing') return
    // Reduced motion disables the exit animation (motion-reduce:animate-none)
    // while the host margin resets instantly (side-panel-host.ts); keeping
    // the 300ms grace would only leave a frozen panel over the already
    // restored host content.
    if (window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches) {
      setPhase('closed')
      return
    }
    const timer = window.setTimeout(() => setPhase('closed'), 300)
    return () => clearTimeout(timer)
  }, [phase])

  if (phase === 'closed') return null
  const closing = phase === 'closing'

  return (
    <div
      data-testid="chat-side-panel"
      className={cn(
        'fixed inset-y-0 right-0 z-50 flex flex-col border-l border-border/50 bg-background shadow-2xl',
        'w-(--rwiki-side-panel-width) max-sm:inset-0 max-sm:w-full max-sm:border-0',
        closing
          ? 'animate-out slide-out-to-right duration-300 motion-reduce:animate-none'
          : 'animate-in slide-in-from-right duration-300 motion-reduce:animate-none',
      )}
    >
      <div
        data-testid="chat-side-panel-header"
        className="flex items-center justify-between border-b border-border/50 px-4 py-2.5"
      >
        <div className="flex items-center gap-2.5">
          <div className="flex size-7 items-center justify-center rounded-lg bg-primary">
            <span className="font-serif text-xs font-bold text-primary-foreground">R</span>
          </div>
          <span className="font-serif text-sm font-medium">{title ?? t.titleDefault}</span>
        </div>
        <Button
          data-testid="chat-side-panel-close"
          size="icon-xs"
          variant="ghost"
          onClick={closeModal}
          aria-label={t.a11yClose}
        >
          <XIcon className="size-3.5" />
        </Button>
      </div>

      <div className="min-h-0 flex-1">
        <ChatPanel showHeader={false} welcomeMessage={welcomeMessage} suggestedQuestions={suggestedQuestions} />
      </div>
    </div>
  )
}
