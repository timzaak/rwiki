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

  it('marks a message that was still streaming when persisted as interrupted on restore', async () => {
    // WHY: 页面在流式中途刷新后，半截回答会被恢复展示；没有标记就无法与正常完成的回答区分，
    // 用户会把截断的内容当成完整答案。恢复时必须保留"未完成"这一事实。
    const updatedAt = Date.now() - 29 * 60 * 1000
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [
            makeMessage({ id: 'user-1', role: 'user', content: 'Q' }),
            makeMessage({
              id: 'asst-1',
              role: 'assistant',
              content: 'partial answ',
              isStreaming: true,
            }),
          ],
          sessionId: 'session-mid-stream',
          updatedAt,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    const messages = useChatStore.getState().messages
    expect(messages[1]).toMatchObject({ isStreaming: false, interrupted: true })
    expect(messages[0].interrupted).toBeUndefined()
  })

  it('does not mark completed messages as interrupted on restore', async () => {
    const updatedAt = Date.now() - 29 * 60 * 1000
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [
            makeMessage({
              id: 'asst-1',
              role: 'assistant',
              content: 'Full answer',
              isStreaming: false,
            }),
          ],
          sessionId: 'session-done',
          updatedAt,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    expect(useChatStore.getState().messages[0].interrupted).toBeUndefined()
  })

  it('keeps the interrupted flag of a previously marked message across another restore', async () => {
    const updatedAt = Date.now() - 29 * 60 * 1000
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [
            makeMessage({
              id: 'asst-1',
              role: 'assistant',
              content: 'partial',
              isStreaming: false,
              interrupted: true,
            }),
          ],
          sessionId: 'session-flagged',
          updatedAt,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    expect(useChatStore.getState().messages[0].interrupted).toBe(true)
  })

  it('does not mark a contentEnded message as interrupted on restore (suggestion-phase refresh)', async () => {
    // WHY: 推荐生成阶段刷新后回看，回答主体已完成的轮次不能带"可能不
    // 完整"标识；contentEnded 也必须随 partialize 持久化，否则刷新即丢失
    // 判定输入。
    const updatedAt = Date.now() - 29 * 60 * 1000
    localStorage.setItem(
      CHAT_STORAGE_KEY,
      JSON.stringify({
        state: {
          messages: [
            makeMessage({
              id: 'user-1',
              role: 'user',
              content: 'Q',
            }),
            makeMessage({
              id: 'asst-1',
              role: 'assistant',
              content: 'full answer',
              isStreaming: true,
              contentEnded: true,
            }),
          ],
          sessionId: 'session-content-ended',
          updatedAt,
        },
        version: 0,
      }),
    )

    await useChatStore.persist.rehydrate()

    const message = useChatStore.getState().messages[1]
    expect(message.isStreaming).toBe(false)
    expect(message.contentEnded).toBe(true)
    expect(message.interrupted).toBeUndefined()
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

describe('useChatStore post-answer suggestions', () => {
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

  it.each([
    ['single assistant message', 1],
    ['multiple assistant messages', 3],
  ] as const)(
    'setLastAssistantSuggestions writes suggestedQuestions on the last assistant message (%s)',
    (_label, assistantCount) => {
      const assistants = Array.from({ length: assistantCount }, (_, i) =>
        makeMessage({
          id: `asst-${i + 1}`,
          role: 'assistant',
          content: `answer ${i + 1}`,
        }),
      )
      useChatStore.setState({
        messages: [
          makeMessage({ id: 'user-1', role: 'user', content: 'Hi' }),
          ...assistants,
        ],
      })

      useChatStore.getState().setLastAssistantSuggestions(['q1', 'q2'])

      const messages = useChatStore.getState().messages
      const lastIndex = messages.length - 1
      expect(messages[lastIndex].suggestedQuestions).toEqual(['q1', 'q2'])
      for (let i = 1; i < assistants.length; i++) {
        expect(messages[lastIndex - i].suggestedQuestions).toBeUndefined()
      }
    },
  )

  it('setLastAssistantSuggestions only writes the last assistant when multiple exist', () => {
    useChatStore.setState({
      messages: [
        makeMessage({ id: 'asst-1', role: 'assistant', content: 'old answer' }),
        makeMessage({
          id: 'user-2',
          role: 'user',
          content: 'follow up',
        }),
        makeMessage({ id: 'asst-2', role: 'assistant', content: 'new answer' }),
      ],
    })

    useChatStore.getState().setLastAssistantSuggestions(['follow-up q'])

    const messages = useChatStore.getState().messages
    expect(messages[2].suggestedQuestions).toEqual(['follow-up q'])
    expect(messages[0].suggestedQuestions).toBeUndefined()
  })

  it('setLastAssistantSuggestions is a no-op when no assistant message exists', () => {
    useChatStore.setState({
      messages: [makeMessage({ id: 'user-1', role: 'user', content: 'Hi' })],
    })

    useChatStore.getState().setLastAssistantSuggestions(['q'])

    const state = useChatStore.getState()
    expect(state.messages[0].suggestedQuestions).toBeUndefined()
    expect(state.messages).toHaveLength(1)
  })

  it('finishStreaming does not clobber an already-written suggestedQuestions', () => {
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'Response text',
          isStreaming: true,
          suggestedQuestions: ['already-set-q'],
        }),
      ],
    })

    useChatStore.getState().finishStreaming()

    const message = useChatStore.getState().messages[0]
    expect(message.isStreaming).toBe(false)
    expect(message.suggestedQuestions).toEqual(['already-set-q'])
  })

  it('persisted assistant message without suggestedQuestions deserializes without error', async () => {
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-05-30T00:30:00Z'))
    try {
      const updatedAt = Date.now() - 29 * 60 * 1000
      localStorage.setItem(
        CHAT_STORAGE_KEY,
        JSON.stringify({
          state: {
            // Old payload shape: no suggestedQuestions field on the assistant.
            messages: [
              makeMessage({
                id: 'asst-legacy',
                role: 'assistant',
                content: 'Legacy answer',
              }),
            ],
            sessionId: 'session-legacy',
            updatedAt,
          },
          version: 0,
        }),
      )

      await useChatStore.persist.rehydrate()

      const message = useChatStore.getState().messages[0]
      expect(message.role).toBe('assistant')
      expect(message.suggestedQuestions).toBeUndefined()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('useChatStore interruption finishing', () => {
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

  it('interruptStreaming marks a streaming message with content as interrupted and clears loading', () => {
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({ id: 'user-1', role: 'user', content: 'Q' }),
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'partial answ',
          isStreaming: true,
        }),
      ],
    })

    useChatStore.getState().interruptStreaming('asst-1')

    expect(useChatStore.getState().messages[1]).toMatchObject({
      content: 'partial answ',
      interrupted: true,
      isStreaming: false,
    })
    expect(useChatStore.getState().isLoading).toBe(false)
  })

  it('interruptStreaming finishes a contentEnded message completely without the interrupted flag', () => {
    // WHY: 回答主体已完成的轮次被打断时，内容是完整的，标成"可能不完
    // 整"会误导用户。
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'full answer',
          isStreaming: true,
          contentEnded: true,
        }),
      ],
    })

    useChatStore.getState().interruptStreaming('asst-1')

    expect(useChatStore.getState().messages[0]).toMatchObject({
      content: 'full answer',
      isStreaming: false,
    })
    expect(useChatStore.getState().messages[0].interrupted).toBeUndefined()
    expect(useChatStore.getState().isLoading).toBe(false)
  })

  it('interruptStreaming removes an empty placeholder that never received content', () => {
    // WHY: 首内容前打断不能留下空占位，否则会被 MessageItem 的 isFailed
    // 分支渲染成失败态。
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({ id: 'user-1', role: 'user', content: 'Q' }),
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: '',
          isStreaming: true,
        }),
      ],
    })

    useChatStore.getState().interruptStreaming('asst-1')

    const state = useChatStore.getState()
    expect(state.messages).toHaveLength(1)
    expect(state.messages[0].role).toBe('user')
    expect(state.isLoading).toBe(false)
  })

  it('interruptStreaming keeps isLoading when a newer assistant turn exists (ownsLoading guard)', () => {
    // WHY: 隐式打断主路径 — 被取代旧流的异步清理按 ID 定向收尾时，不得把
    // 新流的加载状态清掉，否则新回答仍在流式却显示可发送。
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({ id: 'user-1', role: 'user', content: 'First' }),
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'partial first',
          isStreaming: true,
        }),
        makeMessage({ id: 'user-2', role: 'user', content: 'Second' }),
        makeMessage({
          id: 'asst-2',
          role: 'assistant',
          content: '',
          isStreaming: true,
        }),
      ],
    })

    useChatStore.getState().interruptStreaming('asst-1')

    const state = useChatStore.getState()
    expect(state.messages[1].interrupted).toBe(true)
    expect(state.isLoading).toBe(true)
    // The replacement turn is untouched by the old stream's cleanup.
    expect(state.messages[3]).toMatchObject({ isStreaming: true, content: '' })
  })

  it('interruptStreaming is a no-op when the message no longer exists', () => {
    // WHY: 清空会话路径 — 先 stopStreaming 再 clearMessages 后，旧流的收尾
    // 不得复活已清空的会话或误置 isLoading。
    useChatStore.setState({
      isLoading: false,
      messages: [makeMessage({ id: 'user-1', role: 'user', content: 'Q' })],
    })

    useChatStore.getState().interruptStreaming('asst-gone')

    const state = useChatStore.getState()
    expect(state.messages).toHaveLength(1)
    expect(state.isLoading).toBe(false)
    expect(state.error).toBeNull()
  })

  it('markContentEnded records contentEnded by id without touching streaming state', () => {
    useChatStore.setState({
      isLoading: true,
      messages: [
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'answer body',
          isStreaming: true,
        }),
      ],
    })

    useChatStore.getState().markContentEnded('asst-1')

    const message = useChatStore.getState().messages[0]
    expect(message.contentEnded).toBe(true)
    // contentEnd only marks "answer body complete"; the turn is still
    // streaming (suggestions) until done/finishStreaming.
    expect(message.isStreaming).toBe(true)
    expect(useChatStore.getState().isLoading).toBe(true)
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
