import { describe, it, expect, beforeEach, vi } from 'vitest'

import {
  archiveCurrentSession,
  deriveSessionTitle,
  startNewConversation,
  switchToSession,
  useChatHistoryErrorStore,
  useChatHistoryStore,
  type HistorySession,
} from '@/stores/chat-history-store'
import { useChatStore, type ChatMessage } from '@/stores/chat-store'

const HISTORY_STORAGE_KEY = 'rwiki-chat-history'

function makeMessage(overrides?: Partial<ChatMessage>): ChatMessage {
  return {
    id: 'msg-1',
    role: 'user',
    content: 'Hello',
    timestamp: 1000,
    ...overrides,
  }
}

function seedWorkspace(overrides?: {
  conversationId?: string
  sessionId?: string | null
  channelKey?: string | null
  messages?: ChatMessage[]
  updatedAt?: number
  isLoading?: boolean
  error?: string | null
}) {
  useChatStore.setState({
    messages: overrides?.messages ?? [
      makeMessage({ id: 'user-1', role: 'user', content: 'A question' }),
      makeMessage({
        id: 'asst-1',
        role: 'assistant',
        content: 'An answer',
      }),
    ],
    sessionId: overrides?.sessionId ?? 'session-a',
    conversationId: overrides?.conversationId ?? 'conv-a',
    channelKey: overrides?.channelKey ?? '["help_center"]',
    updatedAt: overrides?.updatedAt ?? 2000,
    isLoading: overrides?.isLoading ?? false,
    error: overrides?.error ?? null,
  })
}

// 各 describe 共用的重置：工作区与历史归档/错误 store 全部回到初始态。
beforeEach(() => {
  localStorage.clear()
  useChatStore.setState({
    messages: [],
    sessionId: null,
    conversationId: null,
    channelKey: null,
    updatedAt: null,
    isLoading: false,
    error: null,
  })
  useChatHistoryStore.setState({ sessions: [] })
  useChatHistoryErrorStore.setState({ storageError: false })
})

describe('chat history archiving', () => {
  it('archives the workspace conversation keyed by conversationId and persists it', () => {
    seedWorkspace()

    archiveCurrentSession()

    const sessions = useChatHistoryStore.getState().sessions
    expect(sessions).toHaveLength(1)
    expect(sessions[0]).toMatchObject({
      id: 'conv-a',
      sessionId: 'session-a',
      channelKey: '["help_center"]',
      title: 'A question',
      updatedAt: 2000,
    })
    expect(sessions[0].messages).toHaveLength(2)
    expect(sessions[0].createdAt).toBe(1000)

    const persisted = JSON.parse(localStorage.getItem(HISTORY_STORAGE_KEY) ?? '{}')
    expect(persisted.state.sessions).toHaveLength(1)
  })

  it('re-archiving the same conversation updates it in place instead of duplicating', () => {
    // WHY (US-CORE-048): 归档键必须是 conversationId——否则每次切换/新对话
    // 都会翻倍历史条目，列表被同一会话刷屏。
    seedWorkspace()
    archiveCurrentSession()
    seedWorkspace({
      updatedAt: 3000,
      messages: [
        makeMessage({ id: 'user-1', role: 'user', content: 'A question' }),
        makeMessage({ id: 'asst-1', role: 'assistant', content: 'An answer' }),
        makeMessage({ id: 'user-2', role: 'user', content: 'Follow-up' }),
      ],
    })
    archiveCurrentSession()

    const sessions = useChatHistoryStore.getState().sessions
    expect(sessions).toHaveLength(1)
    expect(sessions[0].messages).toHaveLength(3)
    expect(sessions[0].updatedAt).toBe(3000)
    expect(sessions[0].createdAt).toBe(1000)
  })

  it('archiving an empty workspace is a no-op', () => {
    useChatStore.setState({ messages: [], sessionId: null, conversationId: null })

    archiveCurrentSession()

    expect(useChatHistoryStore.getState().sessions).toHaveLength(0)
  })

  it('sanitizes a still-streaming snapshot the same way as rehydration', () => {
    // WHY: 归档发生在边界事件（新对话/切换）上，此刻回答可能仍在流式——
    // 归档快照必须凝固为"已中断"，否则回看时会渲染一个永远流式中的消息。
    seedWorkspace({
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

    archiveCurrentSession()

    const archived = useChatHistoryStore.getState().sessions[0]
    expect(archived.messages[1]).toMatchObject({
      isStreaming: false,
      interrupted: true,
    })
  })

  it('keeps a contentEnded snapshot un-interrupted when archiving', () => {
    seedWorkspace({
      messages: [
        makeMessage({ id: 'user-1', role: 'user', content: 'Q' }),
        makeMessage({
          id: 'asst-1',
          role: 'assistant',
          content: 'full answer',
          isStreaming: true,
          contentEnded: true,
        }),
      ],
    })

    archiveCurrentSession()

    const archived = useChatHistoryStore.getState().sessions[0]
    expect(archived.messages[1].interrupted).toBeUndefined()
    expect(archived.messages[1].contentEnded).toBe(true)
  })

  it('archives a legacy channelKey-less conversation into the fallback channel', () => {
    // WHY (US-CORE-049): 主站在本 feature 前从不绑定 channelKey，存量会话的
    // 归属无法追溯——归入用户当前所在频道是唯一不丢数据的归属（丢弃违背
    // 永久保留规则）。
    seedWorkspace({ channelKey: null, conversationId: 'conv-legacy' })

    archiveCurrentSession('["help_center"]')

    expect(useChatHistoryStore.getState().sessions[0]).toMatchObject({
      id: 'conv-legacy',
      channelKey: '["help_center"]',
    })
  })

  it('prefers the workspace channelKey over the fallback when both exist', () => {
    seedWorkspace({ channelKey: '["old_channel"]' })

    archiveCurrentSession('["new_channel"]')

    expect(useChatHistoryStore.getState().sessions[0].channelKey).toBe(
      '["old_channel"]',
    )
  })

  it('archives sessions under their channelKey and keeps channels isolated', () => {
    // WHY (US-CORE-049): 频道 A 的历史在频道 B 不可见是硬边界——归档落位
    // 一旦串频道，跨频道用户会看到别人的会话主题。
    seedWorkspace({ conversationId: 'conv-help', channelKey: '["help_center"]' })
    archiveCurrentSession()
    seedWorkspace({
      conversationId: 'conv-dev',
      channelKey: '["developer_docs"]',
      sessionId: 'session-dev',
    })
    archiveCurrentSession()

    const sessions = useChatHistoryStore.getState().sessions
    expect(sessions).toHaveLength(2)
    expect(
      sessions.filter((s) => s.channelKey === '["help_center"]'),
    ).toHaveLength(1)
    expect(
      sessions.filter((s) => s.channelKey === '["developer_docs"]'),
    ).toHaveLength(1)
  })
})

describe('chat history switching', () => {
  it('loads an archived session into the workspace with its sessionId and resets loading/error', () => {
    // WHY (US-CORE-050): 续聊必须透传原 sessionId——装载时丢 id 会让服务端
    // 每次都从零重建上下文，多轮体验退化为单轮。
    seedWorkspace({
      conversationId: 'conv-archived',
      sessionId: 'session-archived',
      updatedAt: 5000,
    })
    archiveCurrentSession()
    // 归档后工作区为空（如已点"新对话"），再从列表装载该会话。
    useChatStore.setState({
      messages: [],
      sessionId: null,
      conversationId: null,
      updatedAt: null,
      isLoading: true,
      error: 'stale error',
    })

    switchToSession('conv-archived')

    const workspace = useChatStore.getState()
    expect(workspace.conversationId).toBe('conv-archived')
    expect(workspace.sessionId).toBe('session-archived')
    expect(workspace.updatedAt).toBe(5000)
    expect(workspace.messages).toHaveLength(2)
    expect(workspace.isLoading).toBe(false)
    expect(workspace.error).toBeNull()
    expect(useChatHistoryStore.getState().sessions).toHaveLength(0)
  })

  it('round-trips A→B→A without losing content in either conversation', () => {
    // WHY (US-CORE-048): 来回切换零丢失是历史回看的核心承诺——任何一次切
    // 换丢消息，用户就不敢再切换，历史功能失去意义。
    const channel = '["help_center"]'
    seedWorkspace({
      conversationId: 'conv-a',
      sessionId: 'session-a',
      channelKey: channel,
      updatedAt: 1000,
    })
    startNewConversation()
    seedWorkspace({
      conversationId: 'conv-b',
      sessionId: 'session-b',
      channelKey: channel,
      updatedAt: 2000,
      messages: [
        makeMessage({ id: 'user-b', role: 'user', content: 'B question' }),
        makeMessage({ id: 'asst-b', role: 'assistant', content: 'B answer' }),
      ],
    })
    startNewConversation()
    expect(useChatHistoryStore.getState().sessions).toHaveLength(2)

    switchToSession('conv-a')
    expect(useChatStore.getState().messages.map((m) => m.content)).toEqual([
      'A question',
      'An answer',
    ])

    switchToSession('conv-b')
    expect(useChatStore.getState().messages.map((m) => m.content)).toEqual([
      'B question',
      'B answer',
    ])
    // B 装载后 A 仍在归档中，未丢失。
    expect(useChatHistoryStore.getState().sessions.map((s) => s.id)).toEqual([
      'conv-a',
    ])

    switchToSession('conv-a')
    expect(useChatStore.getState().messages.map((m) => m.content)).toEqual([
      'A question',
      'An answer',
    ])
  })

  it('is a no-op when the requested session is already the current one', () => {
    seedWorkspace({ conversationId: 'conv-a' })
    archiveCurrentSession()
    const sessionsBefore = useChatHistoryStore.getState().sessions

    switchToSession('conv-a')

    expect(useChatStore.getState().messages).toHaveLength(2)
    expect(useChatHistoryStore.getState().sessions).toBe(sessionsBefore)
  })
})

describe('chat history removal', () => {
  it('removeSession deletes only the targeted session', () => {
    seedWorkspace({ conversationId: 'conv-a' })
    archiveCurrentSession()
    seedWorkspace({ conversationId: 'conv-b' })
    archiveCurrentSession()

    useChatHistoryStore.getState().removeSession('conv-a')

    expect(useChatHistoryStore.getState().sessions.map((s) => s.id)).toEqual([
      'conv-b',
    ])
  })

  it('clearChannel empties only that channel and never touches the workspace', () => {
    // WHY (US-CORE-051): 一键清空的范围 = 当前频道的历史归档；若清空连当前
    // 正在进行的对话一起删，等于把用户正在看的画面撕掉。
    seedWorkspace({ conversationId: 'conv-help', channelKey: '["help_center"]' })
    archiveCurrentSession()
    seedWorkspace({
      conversationId: 'conv-dev',
      channelKey: '["developer_docs"]',
    })
    archiveCurrentSession()
    seedWorkspace({ conversationId: 'conv-current', channelKey: '["help_center"]' })

    useChatHistoryStore.getState().clearChannel('["help_center"]')

    const sessions = useChatHistoryStore.getState().sessions
    expect(sessions.map((s) => s.id)).toEqual(['conv-dev'])
    const workspace = useChatStore.getState()
    expect(workspace.conversationId).toBe('conv-current')
    expect(workspace.messages).toHaveLength(2)
  })
})

describe('chat history storage failures', () => {
  it('surfaces storageError instead of throwing when persisting the archive fails', () => {
    // WHY (US-CORE-049): 配额溢出时聊天主流程绝不能中断——归档保留在内存态
    // 继续工作，失败只变成一条可行动的提示。
    const originalSetItem = Storage.prototype.setItem
    const setItemSpy = vi
      .spyOn(Storage.prototype, 'setItem')
      .mockImplementation(function (
        this: Storage,
        key: string,
        value: string,
      ) {
        if (key === HISTORY_STORAGE_KEY) {
          throw new DOMException('Quota exceeded', 'QuotaExceededError')
        }
        return originalSetItem.call(this, key, value)
      })

    try {
      seedWorkspace()
      expect(() => archiveCurrentSession()).not.toThrow()

      expect(useChatHistoryErrorStore.getState().storageError).toBe(true)
      expect(useChatHistoryStore.getState().sessions).toHaveLength(1)
    } finally {
      setItemSpy.mockRestore()
    }
  })

  it('clears storageError after the next successful archive write', () => {
    const originalSetItem = Storage.prototype.setItem
    const setItemSpy = vi
      .spyOn(Storage.prototype, 'setItem')
      .mockImplementation(function (
        this: Storage,
        key: string,
        value: string,
      ) {
        if (key === HISTORY_STORAGE_KEY) {
          throw new DOMException('Quota exceeded', 'QuotaExceededError')
        }
        return originalSetItem.call(this, key, value)
      })

    try {
      seedWorkspace({ conversationId: 'conv-a' })
      archiveCurrentSession()
      expect(useChatHistoryErrorStore.getState().storageError).toBe(true)
    } finally {
      setItemSpy.mockRestore()
    }

    seedWorkspace({ conversationId: 'conv-b' })
    archiveCurrentSession()
    expect(useChatHistoryErrorStore.getState().storageError).toBe(false)
  })

  it('rehydrates persisted archive sessions on load', async () => {
    const session: HistorySession = {
      id: 'conv-persisted',
      channelKey: '["help_center"]',
      sessionId: 'session-persisted',
      title: 'Persisted question',
      messages: [makeMessage({ role: 'user', content: 'Persisted question' })],
      createdAt: 1000,
      updatedAt: 2000,
    }
    localStorage.setItem(
      HISTORY_STORAGE_KEY,
      JSON.stringify({ state: { sessions: [session] }, version: 0 }),
    )

    await useChatHistoryStore.persist.rehydrate()

    expect(useChatHistoryStore.getState().sessions).toHaveLength(1)
    expect(useChatHistoryStore.getState().sessions[0].id).toBe('conv-persisted')
  })
})

describe('deriveSessionTitle', () => {
  it('uses the first user message collapsed to a single line', () => {
    const messages = [
      makeMessage({ id: 'asst-0', role: 'assistant', content: 'preamble' }),
      makeMessage({ role: 'user', content: 'line one\nline two  spaced' }),
    ]

    expect(deriveSessionTitle(messages)).toBe('line one line two spaced')
  })

  it('truncates long titles at 40 characters', () => {
    expect(deriveSessionTitle([makeMessage({ role: 'user', content: 'x'.repeat(60) })])).toBe(
      `${'x'.repeat(40)}…`,
    )
  })

  it('returns an empty string when no user message exists', () => {
    expect(deriveSessionTitle([makeMessage({ role: 'assistant' })])).toBe('')
  })
})
