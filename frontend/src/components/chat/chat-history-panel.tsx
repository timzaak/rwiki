import { useEffect, useMemo, useState } from 'react'
import { Trash2Icon } from 'lucide-react'

import { formatDate } from '@/lib/format'
import { useChatStore } from '@/stores/chat-store'
import {
  deriveSessionTitle,
  switchToSession,
  useChatHistoryErrorStore,
  useChatHistoryStore,
} from '@/stores/chat-history-store'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { useWidgetI18n } from './widget-i18n'

interface ChatHistoryPanelProps {
  onClose: () => void
}

/**
 * 历史会话列表（消息区覆盖层）：当前会话派生条目（高亮、不可删）+ 当前频道
 * 归档条目（点击切换回看、可删除），按 updatedAt 倒序。纯本地数据，无加载态。
 */
export function ChatHistoryPanel({ onClose }: ChatHistoryPanelProps) {
  const t = useWidgetI18n()
  const sessions = useChatHistoryStore((s) => s.sessions)
  const storageError = useChatHistoryErrorStore((s) => s.storageError)
  const removeSession = useChatHistoryStore((s) => s.removeSession)
  const clearChannel = useChatHistoryStore((s) => s.clearChannel)
  const channelKey = useChatStore((s) => s.channelKey)
  const conversationId = useChatStore((s) => s.conversationId)
  // 按派生值订阅而非整个 messages 数组：流式 chunk 只追加内容，不改变
  // 首条用户消息派生的标题与“有无会话”，避免历史列表随 chunk 重渲染。
  const hasCurrent = useChatStore((s) => s.messages.length > 0)
  const currentTitle = useChatStore((s) => deriveSessionTitle(s.messages))
  const updatedAt = useChatStore((s) => s.updatedAt)
  const [confirmOpen, setConfirmOpen] = useState(false)

  // Escape 收起列表；清空确认框打开时由 ConfirmDialog 接管 Escape（取消）。
  useEffect(() => {
    if (confirmOpen) return
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [confirmOpen, onClose])

  // 同 id 条目以工作区（当前会话）为准，避免边界事件瞬间出现重复条目。
  // memo 化：流式期间 sessions 不变，不重复 filter+sort。
  const archivedSessions = useMemo(
    () =>
      sessions
        .filter((s) => s.channelKey === channelKey && s.id !== conversationId)
        .sort((a, b) => b.updatedAt - a.updatedAt),
    [sessions, channelKey, conversationId],
  )

  return (
    <div
      data-testid="chat-history-panel"
      className="absolute inset-0 z-10 flex flex-col bg-background"
    >
      <div className="flex items-center justify-between border-b border-border/60 px-4 py-2">
        <span className="font-serif text-sm font-medium">{t.historyTitle}</span>
        {archivedSessions.length > 0 && (
          <Button
            data-testid="chat-history-clear-all-button"
            size="xs"
            variant="ghost"
            onClick={() => setConfirmOpen(true)}
          >
            {t.historyClearAll}
          </Button>
        )}
      </div>

      {storageError && (
        <p
          data-testid="chat-history-storage-error"
          className="border-b border-border/40 bg-destructive/5 px-4 py-2 text-xs text-destructive"
        >
          {t.storageError}
        </p>
      )}

      <div
        data-testid="chat-history-list"
        className="flex-1 overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        {hasCurrent && (
          <button
            type="button"
            data-testid="chat-history-item"
            aria-current="true"
            onClick={onClose}
            className="flex w-full flex-col gap-0.5 bg-muted/50 px-4 py-2.5 text-left"
          >
            <span
              data-testid="chat-history-item-title"
              className="truncate text-xs font-medium"
            >
              {currentTitle || t.untitledSession}
            </span>
            {updatedAt && (
              <span className="text-[10px] text-muted-foreground">
                {formatDate(new Date(updatedAt).toISOString())}
              </span>
            )}
          </button>
        )}

        {archivedSessions.map((session) => (
          <div
            key={session.id}
            data-testid="chat-history-item"
            className="group flex items-center border-b border-border/40"
          >
            <button
              type="button"
              onClick={() => {
                switchToSession(session.id)
                onClose()
              }}
              className="flex min-w-0 flex-1 flex-col gap-0.5 px-4 py-2.5 text-left"
            >
              <span
                data-testid="chat-history-item-title"
                className="truncate text-xs"
              >
                {session.title || t.untitledSession}
              </span>
              <span className="text-[10px] text-muted-foreground">
                {formatDate(new Date(session.updatedAt).toISOString())}
              </span>
            </button>
            <Button
              data-testid="chat-history-item-delete-button"
              size="icon-xs"
              variant="ghost"
              className="mr-2 opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
              onClick={() => removeSession(session.id)}
              aria-label={t.a11yDeleteSession}
            >
              <Trash2Icon className="size-3" />
            </Button>
          </div>
        ))}

        {!hasCurrent && archivedSessions.length === 0 && (
          <div
            data-testid="chat-history-empty"
            className="flex flex-1 flex-col items-center justify-center px-6 text-muted-foreground"
          >
            <p className="text-xs opacity-60">{t.historyEmpty}</p>
          </div>
        )}
      </div>

      {confirmOpen && (
        <ConfirmDialog
          title={t.confirmClearTitle}
          description={t.confirmClearBody}
          cancelText={t.confirmClearCancel}
          confirmText={t.confirmClearConfirm}
          onCancel={() => setConfirmOpen(false)}
          onConfirm={() => {
            if (channelKey) clearChannel(channelKey)
            setConfirmOpen(false)
          }}
        />
      )}
    </div>
  )
}
