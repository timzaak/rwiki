import { useEffect, useRef, useState } from 'react'
import { CheckIcon, CopyIcon } from 'lucide-react'

import { cn } from '@/lib/utils'
import { useWidgetI18n } from './widget-i18n'

interface CopyButtonProps {
  text: string
}

const COPIED_RESET_MS = 2000

/**
 * Copy text to the clipboard, falling back to a hidden textarea +
 * execCommand when the async Clipboard API is unavailable — the widget can
 * be embedded into non-secure (http) host pages where navigator.clipboard
 * is undefined.
 */
async function copyText(text: string): Promise<boolean> {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // Permission denied etc. — fall through to the legacy path.
    }
  }

  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.style.position = 'fixed'
  textarea.style.opacity = '0'
  document.body.appendChild(textarea)
  textarea.select()
  try {
    return document.execCommand('copy')
  } catch {
    return false
  } finally {
    textarea.remove()
  }
}

export function CopyButton({ text }: CopyButtonProps) {
  const t = useWidgetI18n()
  const [copied, setCopied] = useState(false)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [])

  async function handleCopy() {
    if (copied) return
    if (!(await copyText(text))) return
    setCopied(true)
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => setCopied(false), COPIED_RESET_MS)
  }

  return (
    <button
      type="button"
      data-testid="message-copy-button"
      className={cn(
        'p-1 rounded-sm transition-colors',
        copied ? 'text-primary' : 'text-muted-foreground/60 hover:text-foreground',
      )}
      onClick={handleCopy}
      aria-label={copied ? t.a11yCopied : t.a11yCopy}
    >
      {copied ? (
        <CheckIcon className="size-3.5" />
      ) : (
        <CopyIcon className="size-3.5" />
      )}
    </button>
  )
}
