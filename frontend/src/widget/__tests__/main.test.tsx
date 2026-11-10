import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

import { useChatStore, useChatModalStore } from '@/stores/chat-store'
import { SIDE_PANEL_OPEN_CLASS, SIDE_PANEL_STYLE_ID } from '../side-panel-host'

// Mock CSS inline imports before importing the module under test
vi.mock('../styles.css?inline', () => ({ default: '/* widget-styles */' }))
vi.mock('highlight.js/styles/github.css?inline', () => ({
  default: '/* hljs-styles */',
}))

// Mock injectStyles to avoid real DOM style injection
vi.mock('../inject-styles', () => ({
  injectStyles: vi.fn(),
}))

// Mock WidgetApp to avoid rendering real component tree
vi.mock('../widget-app', () => ({
  WidgetApp: () => null,
}))

// Mock the stream stop handle: setChannel must abort an in-flight answer
// before clearing the conversation, otherwise stale SSE chunks of the old
// channel bleed into the new one.
const mockStopWidgetStreaming = vi.fn()
vi.mock('../use-widget-chat-stream', () => ({
  stopWidgetStreaming: (...args: unknown[]) => mockStopWidgetStreaming(...args),
}))

// Track createRoot calls
const mockRender = vi.fn()
const mockUnmount = vi.fn()
const mockCreateRoot = vi.fn().mockReturnValue({
  render: mockRender,
  unmount: mockUnmount,
})

vi.mock('react-dom/client', () => ({
  createRoot: (...args: unknown[]) => mockCreateRoot(...args),
}))

// Import after mocks are set up (side-effect module sets window.RWikiChat)
await import('../main')

function getGlobalAPI() {
  return (window as any).RWikiChat as {
    init: (config: any) => void
    destroy: () => void
    setLocale: (locale: string) => void
    ask: (query: string) => void
    setChannel: (channelId: string | string[]) => void
  }
}

function resetStores() {
  useChatStore.setState({
    messages: [],
    sessionId: null,
    error: null,
    isLoading: false,
    pendingQuestion: null,
  })
  useChatModalStore.setState({ isModalOpen: false })
}

describe('Widget lifecycle (init/destroy)', () => {
  beforeEach(() => {
    resetStores()
    // Clean up any leftover widget containers
    document.querySelectorAll('#rwiki-chat-widget').forEach((el) => el.remove())
    mockCreateRoot.mockClear()
    mockRender.mockClear()
    mockUnmount.mockClear()
  })

  afterEach(() => {
    // Ensure cleanup after each test
    const api = getGlobalAPI()
    if (api?.destroy) api.destroy()
  })

  it('init() without apiUrl logs error and does not create DOM', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const api = getGlobalAPI()
    api.init({} as any)

    expect(document.querySelector('#rwiki-chat-widget')).toBeNull()
    expect(mockCreateRoot).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] apiUrl is required')

    errorSpy.mockRestore()
  })

  it('init() without channelId logs error and does not create DOM', () => {
    // Symmetric with the "init without apiUrl" case: channelId is now required,
    // so omitting it must short-circuit validation before any DOM/root work.
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000' } as any)

    expect(document.querySelector('#rwiki-chat-widget')).toBeNull()
    expect(mockCreateRoot).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] channelId is required')

    errorSpy.mockRestore()
  })

  it('init() with valid config creates Shadow DOM container and mounts React', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    const container = document.querySelector('#rwiki-chat-widget')
    expect(container).not.toBeNull()
    expect(container!.shadowRoot).not.toBeNull()

    expect(mockCreateRoot).toHaveBeenCalledTimes(1)
    expect(mockRender).toHaveBeenCalledTimes(1)
  })

  it('destroy() removes container and unmounts React', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    expect(document.querySelector('#rwiki-chat-widget')).not.toBeNull()

    api.destroy()

    expect(document.querySelector('#rwiki-chat-widget')).toBeNull()
    expect(mockUnmount).toHaveBeenCalledTimes(1)
  })

  it('double init() destroys the first instance before creating a new one', () => {
    const api = getGlobalAPI()

    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    api.init({ apiUrl: 'http://localhost:3001', channelId: 'help-center' })

    // Only one container should exist
    expect(document.querySelectorAll('#rwiki-chat-widget')).toHaveLength(1)

    // React root was created twice (once per init)
    expect(mockCreateRoot).toHaveBeenCalledTimes(2)

    // First root was unmounted when second init destroyed it
    expect(mockUnmount).toHaveBeenCalledTimes(1)
  })

  it('destroy() when not initialized is idempotent and throws no error', () => {
    const api = getGlobalAPI()

    // Call destroy without any prior init
    expect(() => api.destroy()).not.toThrow()
    expect(mockUnmount).not.toHaveBeenCalled()
  })
})

describe('Widget setLocale', () => {
  beforeEach(() => {
    // Reset module-level state (container/reactRoot/currentConfig) that the DOM
    // cleanup below cannot reach, so the "setLocale before init" test starts
    // from a truly clean state even if a prior test left the widget mounted.
    getGlobalAPI()?.destroy?.()
    resetStores()
    document.querySelectorAll('#rwiki-chat-widget').forEach((el) => el.remove())
    mockCreateRoot.mockClear()
    mockRender.mockClear()
    mockUnmount.mockClear()
  })

  afterEach(() => {
    const api = getGlobalAPI()
    if (api?.destroy) api.destroy()
  })

  it('setLocale() after init re-renders with the new locale without unmounting', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center', locale: 'en' })
    expect(mockRender).toHaveBeenCalledTimes(1)

    api.setLocale('zh-CN')

    // Re-rendered with the new locale, but the root was not unmounted
    expect(mockRender).toHaveBeenCalledTimes(2)
    expect(mockUnmount).not.toHaveBeenCalled()
    expect(mockCreateRoot).toHaveBeenCalledTimes(1)
    expect(document.querySelector('#rwiki-chat-widget')).not.toBeNull()

    // setLocale actually switched the rendered locale — assert the transition
    // (en -> zh-CN), not just that render() was called again.
    const firstElement = mockRender.mock.calls[0]![0] as {
      props: { config: { locale: string } }
    }
    const lastElement = mockRender.mock.calls.at(-1)![0] as {
      props: { config: { locale: string } }
    }
    expect(firstElement.props.config.locale).toBe('en')
    expect(lastElement.props.config.locale).toBe('zh-CN')
  })

  it('setLocale() before init logs error and does not render', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    const api = getGlobalAPI()
    api.setLocale('zh-CN')

    expect(mockRender).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] setLocale called before init')

    errorSpy.mockRestore()
  })
})

type RenderedConfig = { props: { config: { channelId: string[]; locale: string } } }

function lastRenderedConfig() {
  return (mockRender.mock.calls.at(-1)![0] as RenderedConfig).props.config
}

function seedConversation() {
  useChatStore.setState({
    messages: [
      { id: 'u1', role: 'user', content: 'old question', timestamp: 1 },
      { id: 'a1', role: 'assistant', content: 'old answer', timestamp: 2 },
    ],
    sessionId: 'old-session',
  })
}

describe('Widget ask (US-INTG-017)', () => {
  beforeEach(() => {
    getGlobalAPI()?.destroy?.()
    resetStores()
    document.querySelectorAll('#rwiki-chat-widget').forEach((el) => el.remove())
    mockRender.mockClear()
  })

  afterEach(() => {
    getGlobalAPI()?.destroy?.()
  })

  it('ask() opens the chat and hands the trimmed query to the in-tree sender', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    api.ask('  how to configure rerank  ')

    // The search box hands over a query: the user must see the chat open and
    // the question queued for sending, exactly as typed minus padding.
    expect(useChatModalStore.getState().isModalOpen).toBe(true)
    expect(useChatStore.getState().pendingQuestion).toBe('how to configure rerank')
  })

  it('ask() with a blank query does nothing (the backend rejects empty messages)', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    api.ask('   ')

    expect(useChatModalStore.getState().isModalOpen).toBe(false)
    expect(useChatStore.getState().pendingQuestion).toBeNull()
  })

  it('ask() before init logs error and queues nothing', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    getGlobalAPI().ask('hello')

    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] ask called before init')
    expect(useChatStore.getState().pendingQuestion).toBeNull()
    expect(useChatModalStore.getState().isModalOpen).toBe(false)

    errorSpy.mockRestore()
  })

  it('destroy() drops a question that was queued but never sent', () => {
    // Otherwise the next init() would fire a question the user asked on a
    // widget that no longer exists.
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })
    api.ask('hello')

    api.destroy()

    expect(useChatStore.getState().pendingQuestion).toBeNull()
  })
})

describe('Widget setChannel (US-INTG-016)', () => {
  beforeEach(() => {
    getGlobalAPI()?.destroy?.()
    resetStores()
    document.querySelectorAll('#rwiki-chat-widget').forEach((el) => el.remove())
    mockCreateRoot.mockClear()
    mockRender.mockClear()
    mockUnmount.mockClear()
    mockStopWidgetStreaming.mockClear()
  })

  afterEach(() => {
    getGlobalAPI()?.destroy?.()
  })

  it('setChannel() re-renders with the new channel and starts a new conversation', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center', locale: 'zh-CN' })
    seedConversation()

    api.setChannel('help_center_zh')

    // Live switch: same root, no unmount, new channel, other config kept.
    expect(mockUnmount).not.toHaveBeenCalled()
    expect(mockCreateRoot).toHaveBeenCalledTimes(1)
    expect(lastRenderedConfig().channelId).toEqual(['help_center_zh'])
    expect(lastRenderedConfig().locale).toBe('zh-CN')
    // A conversation never spans channels: the in-flight answer is aborted and
    // the transcript + session are gone (the next turn opens a new session).
    expect(mockStopWidgetStreaming).toHaveBeenCalledTimes(1)
    expect(useChatStore.getState().messages).toEqual([])
    expect(useChatStore.getState().sessionId).toBeNull()
  })

  it('setChannel() with the same channel set keeps the conversation', () => {
    // docs-web calls setChannel on every locale effect run; a repeated value
    // must not wipe the chat the user is in the middle of.
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: ['a', 'b'] })
    seedConversation()
    mockRender.mockClear()

    api.setChannel([' b ', 'a'])

    expect(mockRender).not.toHaveBeenCalled()
    expect(mockStopWidgetStreaming).not.toHaveBeenCalled()
    expect(useChatStore.getState().messages).toHaveLength(2)
    expect(useChatStore.getState().sessionId).toBe('old-session')
  })

  it('init() on another channel drops a persisted foreign-channel session', () => {
    // Full page load of the other language's site: no destroy() runs and the
    // store rehydrates the previous channel's transcript + sessionId. A
    // conversation never spans channels — init must not revive it under the
    // new channel (setChannel already clears on live switches).
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center' })
    api.destroy()
    // Simulate the rehydrated state of the previous site.
    seedConversation()
    useChatStore.setState({ channelKey: JSON.stringify(['help_center']) })

    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center_zh' })

    expect(useChatStore.getState().messages).toEqual([])
    expect(useChatStore.getState().sessionId).toBeNull()
    expect(useChatStore.getState().channelKey).toBe(
      JSON.stringify(['help_center_zh']),
    )
  })

  it('init() with the same channel keeps the persisted session', () => {
    // Reloading the same page must keep the conversation within the TTL.
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center' })
    api.destroy()
    seedConversation()
    useChatStore.setState({ channelKey: JSON.stringify(['help_center']) })

    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center' })

    expect(useChatStore.getState().messages).toHaveLength(2)
    expect(useChatStore.getState().sessionId).toBe('old-session')
  })

  it('setChannel() with an invalid channel logs error and keeps everything', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help_center' })
    seedConversation()
    mockRender.mockClear()

    api.setChannel('   ')

    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] channelId is required')
    expect(mockRender).not.toHaveBeenCalled()
    expect(useChatStore.getState().messages).toHaveLength(2)

    errorSpy.mockRestore()
  })

  it('setChannel() before init logs error and does not render', () => {
    const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

    getGlobalAPI().setChannel('help_center_zh')

    expect(mockRender).not.toHaveBeenCalled()
    expect(errorSpy).toHaveBeenCalledWith('[RWikiChat] setChannel called before init')

    errorSpy.mockRestore()
  })
})

describe('Widget side panel host wiring (US-INTG-018)', () => {
  // main.tsx owns the host DOM, so these tests run the real side-panel-host
  // module against jsdom (only inject-styles/widget-app stay mocked above).
  beforeEach(() => {
    getGlobalAPI()?.destroy?.()
    resetStores()
    document.querySelectorAll('#rwiki-chat-widget').forEach((el) => el.remove())
    document.getElementById(SIDE_PANEL_STYLE_ID)?.remove()
    document.documentElement.classList.remove(SIDE_PANEL_OPEN_CLASS)
  })

  afterEach(() => {
    getGlobalAPI()?.destroy?.()
  })

  it('sidePanel mode: ask() opens the panel and shifts the host document root', () => {
    const api = getGlobalAPI()
    api.init({
      apiUrl: 'http://localhost:3000',
      channelId: 'help-center',
      displayMode: 'sidePanel',
    })

    api.ask('how to configure rerank')

    // ask() routes through the shared isModalOpen state: panel and host shift
    // must open together, with no side-panel-specific API.
    expect(useChatModalStore.getState().isModalOpen).toBe(true)
    expect(document.getElementById(SIDE_PANEL_STYLE_ID)).not.toBeNull()
    expect(document.documentElement.classList.contains(SIDE_PANEL_OPEN_CLASS)).toBe(true)
  })

  it('sidePanel mode: destroy() while open clears the injection synchronously and unsubscribes', () => {
    const api = getGlobalAPI()
    api.init({
      apiUrl: 'http://localhost:3000',
      channelId: 'help-center',
      displayMode: 'sidePanel',
    })
    api.ask('hi')
    expect(document.getElementById(SIDE_PANEL_STYLE_ID)).not.toBeNull()

    api.destroy()

    // Synchronous restore — no animation wait, no residue on the host page.
    expect(document.getElementById(SIDE_PANEL_STYLE_ID)).toBeNull()
    expect(document.documentElement.classList.contains(SIDE_PANEL_OPEN_CLASS)).toBe(false)

    // The subscription is gone too: a later store flip must not re-inject.
    useChatModalStore.getState().openModal()
    expect(document.getElementById(SIDE_PANEL_STYLE_ID)).toBeNull()
    expect(document.documentElement.classList.contains(SIDE_PANEL_OPEN_CLASS)).toBe(false)
  })

  it('modal mode (default): opening the chat leaves the host DOM footprint untouched', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000', channelId: 'help-center' })

    api.ask('hi')

    expect(useChatModalStore.getState().isModalOpen).toBe(true)
    expect(document.getElementById(SIDE_PANEL_STYLE_ID)).toBeNull()
    expect(document.documentElement.classList.contains(SIDE_PANEL_OPEN_CLASS)).toBe(false)
  })
})
