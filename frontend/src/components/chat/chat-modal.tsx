import { useRef, useCallback, useEffect, useState } from 'react'
import { XIcon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { useChatModalStore } from '@/stores/chat-store'
import { ChatPanel } from '@/components/chat/chat-panel'
import { Button } from '@/components/ui/button'

interface ChatModalProps {
  title?: string
  position?: 'left' | 'right'
  welcomeMessage?: string
}

export function ChatModal({ title, position = 'right', welcomeMessage }: ChatModalProps) {
  const isModalOpen = useChatModalStore((s) => s.isModalOpen)
  const closeModal = useChatModalStore((s) => s.closeModal)

  // Drag state — position delta from initial bottom-right anchor
  const [dragOffset, setDragOffset] = useState<{ x: number; y: number }>({
    x: 0,
    y: 0,
  })
  const positionRef = useRef(position)
  positionRef.current = position
  const dragStart = useRef<{
    mouseX: number
    mouseY: number
    offsetX: number
    offsetY: number
  } | null>(null)

  const handlePointerDown = useCallback(
    (e: React.MouseEvent | React.TouchEvent) => {
      // Prevent text selection while dragging
      e.preventDefault()

      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

      dragStart.current = {
        mouseX: clientX,
        mouseY: clientY,
        offsetX: dragOffset.x,
        offsetY: dragOffset.y,
      }
    },
    [dragOffset],
  )

  useEffect(() => {
    if (!isModalOpen) return

    const handlePointerMove = (e: MouseEvent | TouchEvent) => {
      if (!dragStart.current) return

      const clientX = 'touches' in e ? e.touches[0].clientX : e.clientX
      const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY

      const dx = clientX - dragStart.current.mouseX
      const dy = clientY - dragStart.current.mouseY

      const newX = dragStart.current.offsetX + dx
      const newY = dragStart.current.offsetY + dy

      // Clamp within viewport bounds (approximate 400x500 modal)
      const maxX = window.innerWidth - 400
      const maxY = window.innerHeight - 500

      const clampedX = positionRef.current === 'left'
        ? Math.max(0, Math.min(window.innerWidth - 400, newX))
        : Math.max(-window.innerWidth + 100, Math.min(maxX, newX))

      setDragOffset({
        x: clampedX,
        y: Math.max(-window.innerHeight + 100, Math.min(maxY, newY)),
      })
    }

    const handlePointerUp = () => {
      dragStart.current = null
    }

    window.addEventListener('mousemove', handlePointerMove)
    window.addEventListener('mouseup', handlePointerUp)
    window.addEventListener('touchmove', handlePointerMove)
    window.addEventListener('touchend', handlePointerUp)

    return () => {
      window.removeEventListener('mousemove', handlePointerMove)
      window.removeEventListener('mouseup', handlePointerUp)
      window.removeEventListener('touchmove', handlePointerMove)
      window.removeEventListener('touchend', handlePointerUp)
    }
  }, [isModalOpen])

  // Reset drag position when modal closes
  useEffect(() => {
    if (!isModalOpen) {
      setDragOffset({ x: 0, y: 0 })
    }
  }, [isModalOpen])

  if (!isModalOpen) return null

  return (
    <>
      {/* Overlay — does NOT close on click */}
      <div className="fixed inset-0 z-50 bg-black/40" />

      {/* PC: anchored modal */}
      <div
        data-testid="chat-modal"
        className={cn(
          'fixed z-50 flex flex-col overflow-hidden rounded-xl border bg-background shadow-2xl',
          // PC layout
          'bottom-24 h-[500px] w-[400px]',
          position === 'left' ? 'left-6' : 'right-6',
          // Mobile: fullscreen
          'max-sm:inset-0 max-sm:bottom-0 max-sm:right-0 max-sm:left-0 max-sm:h-full max-sm:w-full max-sm:rounded-none max-sm:border-0',
        )}
        style={{
          transform:
            dragOffset.x !== 0 || dragOffset.y !== 0
              ? `translate(${dragOffset.x}px, ${dragOffset.y}px)`
              : undefined,
        }}
      >
        {/* Header / drag handle */}
        <div
          data-testid="chat-modal-header"
          onMouseDown={handlePointerDown}
          onTouchStart={handlePointerDown}
          className="flex cursor-grab items-center justify-between border-b px-4 py-2 active:cursor-grabbing max-sm:cursor-default"
        >
          <span className="text-sm font-medium">{title ?? 'Chat Assistant'}</span>
          <Button
            data-testid="chat-modal-close"
            size="icon-xs"
            variant="ghost"
            onClick={closeModal}
            aria-label="Close chat modal"
          >
            <XIcon className="size-4" />
          </Button>
        </div>

        {/* Body */}
        <div className="min-h-0 flex-1">
          <ChatPanel showHeader={false} welcomeMessage={welcomeMessage} />
        </div>
      </div>
    </>
  )
}
