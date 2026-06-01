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
  error?: string
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
  setSessionId: (id: string) => void
  setError: (error: string) => void
  setLoading: (loading: boolean) => void
  clearMessages: () => void
  removeLastFailedPair: () => void
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
          // Remove the last assistant message (the failed one)
          const lastAssistantIdx = findLastAssistantIndex(messages)
          if (lastAssistantIdx !== -1) {
            messages.splice(lastAssistantIdx, 1)
          }
          // Remove the last user message (the one that triggered the failed response)
          for (let i = messages.length - 1; i >= 0; i--) {
            if (messages[i].role === 'user') {
              messages.splice(i, 1)
              break
            }
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

// --- Modal state (separate store, separate concerns) ---

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
