/**
 * 集中式选择器定义
 *
 * 所有 E2E 测试的元素选择器集中管理在此文件中。
 * 当前端 UI 变更时，只需修改此文件即可。
 *
 * 选择器优先级：
 * 1. data-testid（最稳定，优先使用）
 * 2. Aria roles（语义化）
 * 3. 文本内容（兜底）
 *
 * 根据项目实际情况修改每个选择器。
 */

export const SELECTORS = {
  /** 聊天相关选择器 — 对应 US-CORE-002, US-CORE-003, US-CORE-005, US-CORE-006 */
  chat: {
    panel: '[data-testid="chat-panel"]',
    input: '[data-testid="chat-input"]',
    sendButton: '[data-testid="chat-send-button"]',
    messageList: '[data-testid="message-list"]',
    messageListEmpty: '[data-testid="message-list-empty"]',
    messageItem: (role: string) => `[data-testid="message-item-${role}"]`,
    messageStreaming: '[data-testid="message-item-streaming"]',
    errorBanner: '[data-testid="chat-error-banner"]',
    modal: '[data-testid="chat-modal"]',
    modalHeader: '[data-testid="chat-modal-header"]',
    modalClose: '[data-testid="chat-modal-close"]',
    floatingButton: '[data-testid="floating-chat-button"]',
  },

  /** 通用组件选择器 */
  common: {
    dialog: '[data-testid="dialog"]',
    dialogTitle: '[data-testid="dialog-title"]',
    dialogContent: '[data-testid="dialog-content"]',
    dialogCloseButton: '[data-testid="dialog-close-button"]',
    dialogCancelButton: '[data-testid="dialog-cancel-button"]',
    dialogSubmitButton: '[data-testid="dialog-submit-button"]',

    toast: '[data-testid="toast"], [data-sonner-toast]',
    toastMessage: '[data-testid="toast-message"], [data-sonner-toast] [data-description]',
    successMessage: '[data-testid="success-message"], [data-sonner-toast].success',
    errorMessage: '[data-testid="error-message"], [data-sonner-toast].error',

    loading: '[data-testid="loading"]',
    spinner: '[data-testid="spinner"]',
  },
}

/**
 * 选择器辅助：支持多备选选择器
 */
export function getSelector(selector: string | string[]): string {
  if (Array.isArray(selector)) {
    return selector.join(', ')
  }
  return selector
}
