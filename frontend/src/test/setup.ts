/// <reference types="vitest/globals" />
import '@testing-library/jest-dom/vitest'
import { server } from './mocks/server'

// jsdom 未实现 scrollIntoView，而聊天消息列表挂载即调用：全局补一次，
// 挂载聊天组件树的用例不必各自复制 polyfill。
Element.prototype.scrollIntoView ||= vi.fn()

beforeAll(() => {
  server.listen({ onUnhandledRequest: 'warn' })
})

afterEach(() => {
  server.resetHandlers()
})

afterAll(() => {
  server.close()
})
