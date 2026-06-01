import { MessageCircleIcon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { useChatModalStore } from '@/stores/chat-store'

interface FloatingButtonProps {
  visible?: boolean
  position?: 'left' | 'right'
}

export function FloatingButton({ visible = true, position = 'right' }: FloatingButtonProps) {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const openModal = useChatModalStore((s) => s.openModal)

  if (!visible || isModalOpen) return null

  return (
    <button
      data-testid="floating-chat-button"
      onClick={openModal}
      className={cn(
        'fixed bottom-6 z-50 flex size-14 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg transition-all hover:scale-105 hover:shadow-xl focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 active:scale-95',
        position === 'left' ? 'left-6' : 'right-6',
        'animate-glow-pulse',
      )}
      aria-label="Open chat assistant"
    >
      <MessageCircleIcon className="size-5" />
    </button>
  )
}
