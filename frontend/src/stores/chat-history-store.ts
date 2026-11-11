import { create } from 'zustand'
import {
  createJSONStorage,
  persist,
  type StateStorage,
} from 'zustand/middleware'

import { generateId } from '@/lib/utils'
import {
  sanitizeChatMessages,
  useChatStore,
  type ChatMessage,
} from './chat-store'

const HISTORY_STORAGE_KEY = 'rwiki-chat-history'
const HISTORY_TITLE_MAX_LENGTH = 40

export interface HistorySession {
  /** 与工作区 conversationId 同源：同会话在两个“家”之间的稳定键。 */
  id: string
  /** 频道归属（channelKeyOf 语义）；读取时按当前频道过滤。 */
  channelKey: string | null
  /** 服务端会话 id，续聊透传（可过期，由服务端透明重建）。 */
  sessionId: string | null
  /** 首条用户消息摘要（归档时固化；空串表示无用户消息，展示层兜底占位词条）。 */
  title: string
  messages: ChatMessage[]
  createdAt: number
  updatedAt: number
}

interface ChatHistoryState {
  sessions: HistorySession[]
  upsertSession: (session: HistorySession) => void
  removeSession: (id: string) => void
  clearChannel: (channelKey: string) => void
}

interface ChatHistoryErrorState {
  /** 归档写入失败（如配额溢出）标志：仅驱动面板行内提示。 */
  storageError: boolean
}

// 独立于持久化归档 store：storageError 翻转不得再触发一次全量归档
// 序列化 + localStorage 写（persist 每次 setState 都整体落盘，不做 diff）。
export const useChatHistoryErrorStore = create<ChatHistoryErrorState>(() => ({
  storageError: false,
}))

// 相同值不重复 setState，避免无谓的订阅通知。
function reportStorageWriteResult(failed: boolean) {
  if (useChatHistoryErrorStore.getState().storageError !== failed) {
    useChatHistoryErrorStore.setState({ storageError: failed })
  }
}

// 写入失败（配额溢出）不得中断聊天：归档保留在内存态，仅置 storageError
// 提示用户清理。localStorage 完全不可用（opaque origin）时 getItem 返回
// null，归档库降级为内存态。
const historyStorage: StateStorage = {
  getItem: (name) => {
    try {
      return localStorage.getItem(name)
    } catch {
      return null
    }
  },
  setItem: (name, value) => {
    try {
      localStorage.setItem(name, value)
      reportStorageWriteResult(false)
    } catch {
      reportStorageWriteResult(true)
    }
  },
  removeItem: (name) => {
    try {
      localStorage.removeItem(name)
    } catch {
      // nothing persisted to remove
    }
  },
}

export const useChatHistoryStore = create<ChatHistoryState>()(
  persist(
    (set) => ({
      sessions: [],

      upsertSession: (session) =>
        set((state) => {
          const existing = state.sessions.find((s) => s.id === session.id)
          return {
            sessions: [
              // 重复归档为覆盖更新（updatedAt/title/messages），createdAt 不变。
              existing ? { ...session, createdAt: existing.createdAt } : session,
              ...state.sessions.filter((s) => s.id !== session.id),
            ],
          }
        }),

      removeSession: (id) =>
        set((state) => ({ sessions: state.sessions.filter((s) => s.id !== id) })),

      // 清空范围 = 单个频道的归档条目；工作区当前会话永不在此路径中被触碰。
      clearChannel: (channelKey) =>
        set((state) => ({
          sessions: state.sessions.filter((s) => s.channelKey !== channelKey),
        })),
    }),
    {
      name: HISTORY_STORAGE_KEY,
      storage: createJSONStorage(() => historyStorage),
      partialize: (state) => ({ sessions: state.sessions }),
    },
  ),
)

let localStorageAvailable: boolean | null = null

// 一次性探测（含写入）：受限环境（opaque origin 等）不提供历史入口，
// 聊天降级为现状单会话行为。
export function isHistoryStorageAvailable(): boolean {
  if (localStorageAvailable === null) {
    try {
      const probeKey = '__rwiki_storage_probe__'
      localStorage.setItem(probeKey, '1')
      localStorage.removeItem(probeKey)
      localStorageAvailable = true
    } catch {
      localStorageAvailable = false
    }
  }
  return localStorageAvailable
}

export function deriveSessionTitle(messages: ChatMessage[]): string {
  const firstUser = messages.find((m) => m.role === 'user')
  if (!firstUser) return ''
  const flattened = firstUser.content.replace(/\s+/g, ' ').trim()
  return flattened.length > HISTORY_TITLE_MAX_LENGTH
    ? `${flattened.slice(0, HISTORY_TITLE_MAX_LENGTH)}…`
    : flattened
}

/**
 * 把工作区当前会话归档进历史库（upsert，键 = conversationId）。
 * 工作区本身不动（是否清空由调用方决定）；空工作区 no-op。
 * fallbackChannelKey：工作区 channelKey 为 null 的遗留会话归入调用方
 * 即将绑定的频道（归属无法追溯，丢弃违背永久保留决策）。
 */
export function archiveCurrentSession(fallbackChannelKey?: string): void {
  const workspace = useChatStore.getState()
  if (workspace.messages.length === 0) return
  useChatHistoryStore.getState().upsertSession({
    id: workspace.conversationId ?? generateId(),
    channelKey: workspace.channelKey ?? fallbackChannelKey ?? null,
    sessionId: workspace.sessionId,
    title: deriveSessionTitle(workspace.messages),
    // 归档快照与工作区 rehydrate 共用同一净化（isStreaming → false、
    // 未收 contentEnd 的流式消息推导为 interrupted）。
    messages: sanitizeChatMessages(workspace.messages),
    createdAt: workspace.messages[0]?.timestamp ?? Date.now(),
    updatedAt: workspace.updatedAt ?? Date.now(),
  })
}

/** “新对话”：当前会话归档后开启空会话。 */
export function startNewConversation(): void {
  archiveCurrentSession()
  useChatStore.getState().clearMessages()
}

/**
 * 切换到归档会话：先归档当前工作区，再把目标条目移出归档库、整体装载
 * 回工作区（isLoading/error 复位）。目标不存在（如已是当前会话）时 no-op。
 */
export function switchToSession(id: string): void {
  if (useChatStore.getState().conversationId === id) return
  archiveCurrentSession()
  const history = useChatHistoryStore.getState()
  const target = history.sessions.find((s) => s.id === id)
  if (!target) return
  history.removeSession(id)
  useChatStore.getState().loadSession({
    messages: target.messages,
    sessionId: target.sessionId,
    conversationId: target.id,
    updatedAt: target.updatedAt,
  })
}

/**
 * 换频道协议（widget bindChannel 与主站 /c/$channelId 绑定共用）：会话属于
 * 其频道。channelKey 变化时当前会话先归档进其频道的历史（channelKey 为 null
 * 的遗留会话归入目标频道），再清空工作区并绑定新 key，防止异频道的持久化
 * 会话在新频道下复活。key 未变化时 no-op（不误清当前会话）。
 */
export function rebindWorkspaceChannel(nextChannelKey: string): void {
  const store = useChatStore.getState()
  if (store.channelKey === nextChannelKey) return
  archiveCurrentSession(nextChannelKey)
  store.clearMessages()
  store.setChannelKey(nextChannelKey)
}
