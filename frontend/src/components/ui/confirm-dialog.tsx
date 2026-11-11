import {
  useEffect,
  useRef,
  type KeyboardEvent as ReactKeyboardEvent,
} from 'react'

import { Button } from '@/components/ui/button'

interface ConfirmDialogProps {
  title: string
  description?: string
  cancelText: string
  confirmText: string
  onCancel: () => void
  onConfirm: () => void
}

/**
 * 通用确认对话框原语：受控纯展示（标题/说明/文案/回调）。
 * 内联渲染、不 portal——widget 场景 portal 会逃逸 Shadow DOM 破坏样式隔离。
 * 初始焦点在“取消”按钮（防误触回车确认）；Escape = 取消；Tab 在两个按钮间循环。
 */
export function ConfirmDialog({
  title,
  description,
  cancelText,
  confirmText,
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  const cancelButtonRef = useRef<HTMLButtonElement>(null)
  const confirmButtonRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    cancelButtonRef.current?.focus()
  }, [])

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onCancel()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [onCancel])

  function handleTab(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'Tab') return
    const first = cancelButtonRef.current
    const last = confirmButtonRef.current
    if (!first || !last) return
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }

  return (
    <div
      data-testid="dialog"
      role="dialog"
      aria-modal="true"
      aria-label={title}
      onKeyDown={handleTab}
      className="absolute inset-0 z-20 flex items-center justify-center bg-black/30 p-6"
    >
      <div className="w-full max-w-xs rounded-xl border border-border/60 bg-background p-4 shadow-lg">
        <h3 data-testid="dialog-title" className="text-sm font-medium">
          {title}
        </h3>
        {description && (
          <p
            data-testid="dialog-content"
            className="mt-1.5 text-xs leading-relaxed text-muted-foreground"
          >
            {description}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <Button
            ref={cancelButtonRef}
            data-testid="dialog-cancel-button"
            size="sm"
            variant="outline"
            onClick={onCancel}
          >
            {cancelText}
          </Button>
          <Button
            ref={confirmButtonRef}
            data-testid="dialog-submit-button"
            size="sm"
            variant="destructive"
            onClick={onConfirm}
          >
            {confirmText}
          </Button>
        </div>
      </div>
    </div>
  )
}
