import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

import { useChatStore, useChatModalStore } from '@/stores/chat-store'

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
  }
}

function resetStores() {
  useChatStore.setState({
    messages: [],
    sessionId: null,
    error: null,
    isLoading: false,
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
