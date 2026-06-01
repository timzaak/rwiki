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

  it('init() with valid config creates Shadow DOM container and mounts React', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000' })

    const container = document.querySelector('#rwiki-chat-widget')
    expect(container).not.toBeNull()
    expect(container!.shadowRoot).not.toBeNull()

    expect(mockCreateRoot).toHaveBeenCalledTimes(1)
    expect(mockRender).toHaveBeenCalledTimes(1)
  })

  it('destroy() removes container and unmounts React', () => {
    const api = getGlobalAPI()
    api.init({ apiUrl: 'http://localhost:3000' })

    expect(document.querySelector('#rwiki-chat-widget')).not.toBeNull()

    api.destroy()

    expect(document.querySelector('#rwiki-chat-widget')).toBeNull()
    expect(mockUnmount).toHaveBeenCalledTimes(1)
  })

  it('double init() destroys the first instance before creating a new one', () => {
    const api = getGlobalAPI()

    api.init({ apiUrl: 'http://localhost:3000' })

    api.init({ apiUrl: 'http://localhost:3001' })

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
