import { create } from 'zustand'
import { persist } from 'zustand/middleware'

const CHAT_STORAGE_KEY = 'rwiki-chat-state'
const CHAT_STORAGE_TTL_MS = 30 * 60 * 1000

function findLastAssistantIndex(messages: ChatMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i].role === 'assistant') return i
  }
  return -1
}

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: number
  isStreaming?: boolean
  /** 用户主动打断（停止/新发送/unmount）或恢复时由 isStreaming=true 推导：内容不完整。 */
  interrupted?: boolean
  /** 本轮已收到 contentEnd SSE 事件：回答主体已完成（推荐生成阶段）。 */
  contentEnded?: boolean
  error?: string
  feedback?: 'like' | 'dislike'
  suggestedQuestions?: string[]
}

interface ChatState {
  messages: ChatMessage[]
  sessionId: string | null
  updatedAt: number | null
  isLoading: boolean
  error: string | null

  addUserMessage: (content: string) => void
  addAssistantMessage: (id: string) => void
  appendToLastAssistant: (chunk: string) => void
  finishStreaming: () => void
  markContentEnded: (messageId: string) => void
  interruptStreaming: (messageId: string) => void
  setSessionId: (id: string) => void
  setError: (error: string) => void
  setLoading: (loading: boolean) => void
  clearMessages: () => void
  removeLastFailedPair: () => void
  updateMessageFeedback: (messageId: string, feedback: 'like' | 'dislike' | undefined) => void
  setLastAssistantSuggestions: (questions: string[]) => void
}

type PersistedChatState = Pick<ChatState, 'messages' | 'sessionId' | 'updatedAt'>

function removePersistedChatState() {
  localStorage.removeItem(CHAT_STORAGE_KEY)
}

function sanitizePersistedState(
  state: PersistedChatState,
): PersistedChatState | null {
  if (!state.updatedAt || Date.now() - state.updatedAt > CHAT_STORAGE_TTL_MS) {
    removePersistedChatState()
    return null
  }

  return {
    ...state,
    messages: state.messages.map((message) => ({
      ...message,
      isStreaming: false,
      interrupted:
        message.isStreaming && !message.contentEnded
          ? true
          : message.interrupted,
    })),
  }
}

export const useChatStore = create<ChatState>()(
  persist(
    (set) => ({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,

      addUserMessage: (content) =>
        set((state) => {
          const now = Date.now()
          return {
            messages: [
              ...state.messages,
              {
                id: crypto.randomUUID(),
                role: 'user' as const,
                content,
                timestamp: now,
              },
            ],
            updatedAt: now,
            error: null,
          }
        }),

      addAssistantMessage: (id) =>
        set((state) => {
          const now = Date.now()
          return {
            messages: [
              ...state.messages,
              {
                id,
                role: 'assistant' as const,
                content: '',
                timestamp: now,
                isStreaming: true,
              },
            ],
            updatedAt: now,
            isLoading: true,
          }
        }),

      appendToLastAssistant: (chunk) =>
        set((state) => {
          const messages = [...state.messages]
          const lastIndex = findLastAssistantIndex(messages)
          if (lastIndex === -1) return state
          messages[lastIndex] = {
            ...messages[lastIndex],
            content: messages[lastIndex].content + chunk,
          }
          return { messages, updatedAt: Date.now() }
        }),

      finishStreaming: () =>
        set((state) => {
          const messages = [...state.messages]
          const lastIndex = findLastAssistantIndex(messages)
          if (lastIndex === -1) return { ...state, isLoading: false }
          messages[lastIndex] = {
            ...messages[lastIndex],
            isStreaming: false,
          }
          return { messages, updatedAt: Date.now(), isLoading: false }
        }),

      markContentEnded: (messageId) =>
        set((state) => {
          if (!state.messages.some((m) => m.id === messageId)) return state
          const messages = state.messages.map((msg) =>
            msg.id === messageId ? { ...msg, contentEnded: true } : msg,
          )
          return { messages, updatedAt: Date.now() }
        }),

      // 用户主动打断时按消息 ID 收尾该轮（打断不是错误，不 setError）：
      // - 无内容且未收到 contentEnd → 移除空占位（避免被 isFailed 渲染成失败态）
      // - 已 contentEnded → 完整收尾、无中断标识
      // - 其余 → interrupted: true
      // 仅当该消息之后不存在更新的 assistant 轮次时才复位 isLoading（ownsLoading 守卫），
      // 防止被取代旧流的异步清理清掉新流的加载状态。
      interruptStreaming: (messageId) =>
        set((state) => {
          const idx = state.messages.findIndex((m) => m.id === messageId)
          if (idx === -1) return state
          const message = state.messages[idx]
          const ownsLoading = !state.messages
            .slice(idx + 1)
            .some((m) => m.role === 'assistant')
          let messages: ChatMessage[]
          if (!message.content.trim() && !message.contentEnded) {
            messages = state.messages.filter((m) => m.id !== messageId)
          } else {
            messages = [...state.messages]
            messages[idx] = {
              ...message,
              isStreaming: false,
              ...(message.contentEnded ? {} : { interrupted: true }),
            }
          }
          return {
            messages,
            updatedAt: Date.now(),
            ...(ownsLoading ? { isLoading: false } : {}),
          }
        }),

      setSessionId: (id) => set({ sessionId: id, updatedAt: Date.now() }),

      setError: (error) =>
        set({
          error,
          isLoading: false,
        }),

      setLoading: (loading) => set({ isLoading: loading }),

      clearMessages: () => {
        set({
          messages: [],
          sessionId: null,
          updatedAt: null,
          isLoading: false,
          error: null,
        })
        removePersistedChatState()
      },

      removeLastFailedPair: () =>
        set((state) => {
          const messages = [...state.messages]
          const lastAssistantIdx = findLastAssistantIndex(messages)
          if (lastAssistantIdx !== -1) {
            messages.splice(lastAssistantIdx, 1)
          }
          for (let i = messages.length - 1; i >= 0; i--) {
            if (messages[i].role === 'user') {
              messages.splice(i, 1)
              break
            }
          }
          return { messages, updatedAt: Date.now() }
        }),

      updateMessageFeedback: (messageId, feedback) =>
        set((state) => {
          if (!state.messages.some((m) => m.id === messageId)) return state
          const messages = state.messages.map((msg) =>
            msg.id === messageId ? { ...msg, feedback } : msg,
          )
          return { messages, updatedAt: Date.now() }
        }),

      setLastAssistantSuggestions: (questions) =>
        set((state) => {
          const messages = [...state.messages]
          const lastIndex = findLastAssistantIndex(messages)
          if (lastIndex === -1) return state
          messages[lastIndex] = {
            ...messages[lastIndex],
            suggestedQuestions: questions,
          }
          return { messages, updatedAt: Date.now() }
        }),
    }),
    {
      name: CHAT_STORAGE_KEY,
      partialize: (state): PersistedChatState => ({
        messages: state.messages,
        sessionId: state.sessionId,
        updatedAt: state.updatedAt,
      }),
      merge: (persistedState, currentState) => {
        const persisted = persistedState as PersistedChatState | undefined
        if (!persisted) return currentState

        const sanitized = sanitizePersistedState(persisted)
        if (!sanitized) return currentState

        return {
          ...currentState,
          ...sanitized,
          isLoading: false,
          error: null,
        }
      },
    },
  ),
)

interface ChatModalState {
  isModalOpen: boolean
  openModal: () => void
  closeModal: () => void
  toggleModal: () => void
}

export const useChatModalStore = create<ChatModalState>((set) => ({
  isModalOpen: false,
  openModal: () => set({ isModalOpen: true }),
  closeModal: () => set({ isModalOpen: false }),
  toggleModal: () => set((state) => ({ isModalOpen: !state.isModalOpen })),
}))
