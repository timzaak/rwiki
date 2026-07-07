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
    suggestedQuestions: '[data-testid="suggested-questions"]',
    // The empty-state container, rendered only while messages.length === 0
    // (chat-panel.tsx). Distinct from the per-message follow-up suggestions
    // (message-item.tsx, gated on [chat] enable_post_answer_suggestions).
    emptyStateSuggestions: '[data-testid="chat-empty-suggestions"] [data-testid="suggested-questions"]',
    suggestedQuestionButton: '[data-testid="suggested-question-button"]',
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

  /**
   * 多频道相关选择器（multi-channel）— 对应 US-INTG-005/007。
   *
   * 频道入口列表 testid 来源：`frontend/src/routes/index.tsx`
   *   - `channel-list-loading` / `channel-list-error` / `channel-list-empty`
   *   - `channel-entry`（每个频道链接内的名称 span）
   *   - `channel-entry-${channelId}`（频道链接本身，按 id 区分）
   * 频道路由 testid 来源：`frontend/src/routes/c/$channelId.tsx`
   *   - `channel-loading`（listChannels 校验中）
   *   - `channel-error`（频道列表加载失败，含重试）
   *   - `channel-not-found`（未知频道）
   *
   * 注意：`admin-channel-select`（管理后台上传频道选择器）不在 demo 范围内
   * （由 FE 单元测试 FE-T03 覆盖），故此处不收录。
   */
  channel: {
    channelEntry: '[data-testid="channel-entry"]',
    channelEntryById: (id: string) => `[data-testid="channel-entry-${id}"]`,
    channelListLoading: '[data-testid="channel-list-loading"]',
    channelListError: '[data-testid="channel-list-error"]',
    channelListEmpty: '[data-testid="channel-list-empty"]',
    channelLoading: '[data-testid="channel-loading"]',
    channelError: '[data-testid="channel-error"]',
    channelNotFound: '[data-testid="channel-not-found"]',
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
