import { describe, it, expect, beforeEach } from 'vitest'
import { useChatStore, useChatModalStore } from '@/stores/chat-store'
import type { ChatMessage } from '@/stores/chat-store'

const CHAT_STORAGE_KEY = 'rwiki-chat-state'

function makeMessage(overrides?: Partial<ChatMessage>): ChatMessage {
  return {
    id: 'msg-1',
    role: 'user',
    content: 'Hello',
    timestamp: Date.now(),
    ...overrides,
  }
}

describe('useChatStore message management', () => {
  beforeEach(() => {
    localStorage.clear()
    useChatStore.setState({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,
    })
  })

  it('addUserMessage appends a user message with correct role/content/timestamp and clears error', () => {
    useChatStore.setState({ error: 'previous error' })
    const before = Date.now()

    useChatStore.getState().addUserMessage('Hello world')

    const after = Date.now()
    const state = useChatStore.getState()

    expect(state.messages).toHaveLength(1)
    expect(state.messages[0].role).toBe('user')
    expect(state.messages[0].content).toBe('Hello world')
    expect(state.messages[0].timestamp).toBeGreaterThanOrEqual(before)
    expect(state.messages[0].timestamp).toBeLessThanOrEqual(after)
    expect(state.error).toBeNull()
  })

  it('addAssistantMessage appends a placeholder assistant message with isStreaming=true and sets isLoading=true', () => {
    useChatStore.getState().addAssistantMessage('asst-1')

    const state = useChatStore.getState()

    expect(state.messages).toHaveLength(1)
    expect(state.messages[0]).toMatchObject({
      id: 'asst-1',
      role: 'assistant',
      content: '',
      isStreaming: true,
    })
    expect(state.isLoading).toBe(true)
  })

  it('appendToLastAssistant appends chunk content to the last assistant message', () => {
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'msg-1', role: 'user', content: 'Hi' }),
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'Hello',
        }),
      ],
    })

    useChatStore.getState().appendToLastAssistant(' world')

    const messages = useChatStore.getState().messages
    expect(messages[1].content).toBe('Hello world')
  })

  it('finishStreaming sets isStreaming=false on last assistant message and isLoading=false', () => {
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'Response text',
          isStreaming: true,
        }),
      ],
    })

    useChatStore.getState().finishStreaming()

    const state = useChatStore.getState()
    expect(state.messages[0].isStreaming).toBe(false)
    expect(state.isLoading).toBe(false)
  })

  it('clearMessages resets messages, sessionId, and error to initial state', () => {
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'msg-1' }),
        makeMessage({ id: 'msg-2' }),
      ],
      sessionId: 'session-abc',
      updatedAt: Date.now(),
      isLoading: true,
      error: 'some error',
    })

    useChatStore.getState().clearMessages()

    const state = useChatStore.getState()
    expect(state.messages).toEqual([])
    expect(state.sessionId).toBeNull()
    expect(state.updatedAt).toBeNull()
    expect(state.isLoading).toBe(false)
    expect(state.error).toBeNull()
    expect(localStorage.getItem(CHAT_STORAGE_KEY)).toBeNull()
  })
})

describe('useChatStore session management', () => {
  beforeEach(() => {
    localStorage.clear()
    useChatStore.setState({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,
    })
  })

  it('setSessionId stores the session ID', () => {
    useChatStore.getState().setSessionId('session-123')

    expect(useChatStore.getState().sessionId).toBe('session-123')
  })

  it('clearMessages also clears sessionId', () => {
    useChatStore.getState().setSessionId('session-xyz')
    useChatStore.getState().clearMessages()

    expect(useChatStore.getState().sessionId).toBeNull()
  })
})

describe('useChatStore error handling', () => {
  beforeEach(() => {
    localStorage.clear()
    useChatStore.setState({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,
    })
  })

  it('setError sets error string and isLoading=false', () => {
    useChatStore.setState({ isLoading: true })

    useChatStore.getState().setError('Network failure')

    const state = useChatStore.getState()
    expect(state.error).toBe('Network failure')
    expect(state.isLoading).toBe(false)
  })

  it('addUserMessage clears any existing error', () => {
    useChatStore.setState({ error: 'previous error' })

    useChatStore.getState().addUserMessage('New message')

    expect(useChatStore.getState().error).toBeNull()
  })
})

describe('useChatStore loading state', () => {
  beforeEach(() => {
    localStorage.clear()
    useChatStore.setState({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,
    })
  })

  it('setLoading toggles isLoading', () => {
    useChatStore.getState().setLoading(true)
    expect(useChatStore.getState().isLoading).toBe(true)

    useChatStore.getState().setLoading(false)
    expect(useChatStore.getState().isLoading).toBe(false)
  })

  it('addAssistantMessage sets isLoading=true', () => {
    useChatStore.getState().addAssistantMessage('asst-1')

    expect(useChatStore.getState().isLoading).toBe(true)
  })

  it('finishStreaming sets isLoading=false', () => {
    useChatStore.setState({ isLoading: true })

    useChatStore.getState().finishStreaming()

    expect(useChatStore.getState().isLoading).toBe(false)
  })

  it('setError sets isLoading=false', () => {
    useChatStore.setState({ isLoading: true })

    useChatStore.getState().setError('Something went wrong')

    expect(useChatStore.getState().isLoading).toBe(false)
  })
})

describe('useChatStore persistence', () => {
  beforeEach(() => {
    localStorage.clear()
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-05-30T00:30:00Z'))
    useChatStore.setState({
      messages: [],
      sessionId: null,
      updatedAt: null,
      isLoading: false,
      error: null,
    })
  })

  afterEach(() => {
    vi.useRealTimers()
    localStorage.clear()
  })

  it('restores a conversation updated less than 30 minutes ago', async () => {
    const updatedAt = Date.now() - 29 * 60 * 1000
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [
            makeMessage({
              id: 'asst-1',
              role: 'assistant',
              content: 'Recent answer',
              isStreaming: true,
            }),
          ],
          sessionId: 'session-recent',
          updatedAt,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    const state = useChatStore.getState()
    expect(state.messages).toHaveLength(1)
    expect(state.messages[0]).toMatchObject({
      content: 'Recent answer',
      isStreaming: false,
    })
    expect(state.sessionId).toBe('session-recent')
    expect(state.updatedAt).toBe(updatedAt)
    expect(state.isLoading).toBe(false)
    expect(state.error).toBeNull()
  })

  it('drops a conversation updated more than 30 minutes ago', async () => {
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [makeMessage({ content: 'Expired question' })],
          sessionId: 'session-expired',
          updatedAt: Date.now() - 31 * 60 * 1000,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    const state = useChatStore.getState()
    expect(state.messages).toEqual([])
    expect(state.sessionId).toBeNull()
    expect(state.updatedAt).toBeNull()
    expect(state.isLoading).toBe(false)
    expect(state.error).toBeNull()
    expect(localStorage.getItem(CHAT_STORAGE_KEY)).toBeNull()
  })
})

describe('useChatModalStore modal state', () => {
  beforeEach(() => {
    useChatModalStore.setState({ isModalOpen: false })
  })

  it('initial state has isModalOpen=false', () => {
    useChatModalStore.setState({ isModalOpen: false })

    expect(useChatModalStore.getState().isModalOpen).toBe(false)
  })

  it('openModal sets isModalOpen=true', () => {
    useChatModalStore.getState().openModal()

    expect(useChatModalStore.getState().isModalOpen).toBe(true)
  })

  it('closeModal sets isModalOpen=false', () => {
    useChatModalStore.setState({ isModalOpen: true })

    useChatModalStore.getState().closeModal()

    expect(useChatModalStore.getState().isModalOpen).toBe(false)
  })

  it('toggleModal flips isModalOpen', () => {
    expect(useChatModalStore.getState().isModalOpen).toBe(false)

    useChatModalStore.getState().toggleModal()
    expect(useChatModalStore.getState().isModalOpen).toBe(true)

    useChatModalStore.getState().toggleModal()
    expect(useChatModalStore.getState().isModalOpen).toBe(false)
  })
})
