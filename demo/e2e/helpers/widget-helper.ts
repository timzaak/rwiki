/* global window, document */
/**
 * Shared Widget Shadow-DOM helpers for the standalone IIFE bundle.
 *
 * The widget mounts inside a Shadow DOM (`#rwiki-chat-widget` →
 * `container.attachShadow({ mode: 'open' })`). Playwright `locator()`
 * does NOT pierce open shadow roots, so every query here goes through
 * `page.evaluate()` and reads from `host.shadowRoot`.
 *
 * These helpers are shared by:
 *  - demo/e2e/widget-suggested-questions.e2e.ts (US-INTG-004 suggestions)
 *  - demo/e2e/widget-multi-channel.e2e.ts (US-INTG-006 channelId init contract)
 *
 * No hardcoded selector strings beyond the central `[data-testid=...]`
 * literals already used by these widget tests (they live entirely inside
 * the Shadow DOM and are not part of SELECTORS, which targets the
 * main-route `/c/$channelId` page). Each literal maps 1:1 to a frontend
 * `data-testid` — see the inline comments for the source file.
 */

import type { Page } from '@playwright/test'

/**
 * Demo backend (port 18080). Serves the built widget bundle at
 * `/widget/rwiki-chat.js`. NOT the old 8080.
 */
export const WIDGET_BASE_URL = process.env.BASE_URL || 'http://localhost:18080'

/**
 * Default channel for the positive widget scenarios. `help_center` has a
 * seeded KB (via scripts/demo-start.py) and configured suggested_questions.
 */
export const DEFAULT_WIDGET_CHANNEL_ID = 'help_center'

/**
 * A channel that is configured but intentionally left with no KB / no
 * suggested_questions — used to exercise the empty-array contract.
 */
export const EMPTY_WIDGET_CHANNEL_ID = 'developer_docs'

export interface WidgetInitConfig {
  apiUrl: string
  /** 频道标识；可传单个字符串或字符串数组（多频道并集检索）。 */
  channelId?: string | string[]
  [key: string]: unknown
}

/**
 * Load the built widget IIFE bundle into a fresh page and initialize it.
 *
 * `page.setContent` is used (NOT ChatPage / FRONTEND_URL) because these
 * tests target the standalone widget bundle hosted by the demo backend,
 * embedded into an arbitrary host page — exactly the integrator scenario.
 *
 * @param page      Playwright Page
 * @param config    Config object passed verbatim to `RWikiChat.init(config)`.
 *                  Callers decide whether to include `channelId`.
 */
export async function setupWidgetPage(
  page: Page,
  config: WidgetInitConfig,
): Promise<void> {
  await page.setContent(`
    <!DOCTYPE html><html><body>
    <script src="${config.apiUrl}/widget/rwiki-chat.js"></script>
    </body></html>
  `)
  await page.evaluate((cfg) => {
    ;(window as any).RWikiChat.init(cfg)
  }, config)
}

/**
 * Shadow-DOM query helpers. Every method uses `page.evaluate()` because
 * Playwright locators cannot pierce shadow roots.
 *
 * All selectors are `[data-testid=...]` literals that exist in
 * `frontend/src/components/chat/*`:
 *  - floating-chat-button  : floating-button.tsx
 *  - chat-modal            : chat-modal.tsx
 *  - chat-input            : chat-input.tsx
 *  - chat-send-button      : chat-input.tsx
 *  - message-item-assistant: message-item.tsx  (role="assistant")
 *  - message-item-streaming: message-item.tsx
 *  - suggested-questions   : suggested-questions.tsx
 *  - suggested-question-button: suggested-questions.tsx
 *  - message-retry-button  : message-item.tsx (rendered on failed responses)
 */
export const createWidgetHelper = (page: Page) => ({
  /** true if the `#rwiki-chat-widget` host element exists in the document. */
  async hasHost(): Promise<boolean> {
    return page.evaluate(
      () => document.getElementById('rwiki-chat-widget') !== null,
    )
  },

  /**
   * true if the host has a shadowRoot with chat content. Used to assert the
   * "does not render" negative case (no host → no shadowRoot chat content).
   */
  async hasShadowChatContent(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      if (!root) return false
      // The floating button is the first stable element the widget renders.
      return root.querySelector('[data-testid="floating-chat-button"]') !== null
    })
  },

  /** Click the floating chat button inside the Shadow DOM to open the modal. */
  async clickFloatingButton(): Promise<void> {
    await page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const btn = root?.querySelector(
        '[data-testid="floating-chat-button"]',
      ) as HTMLElement | null
      btn?.click()
    })
  },

  /** true if the chat modal is present inside the Shadow DOM. */
  async isModalOpen(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-modal"]') != null
    })
  },

  /** Wait until the chat modal is present in the Shadow DOM. */
  async waitForModal(timeout = 10000): Promise<void> {
    await page.waitForFunction(
      () => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        return root?.querySelector('[data-testid="chat-modal"]') != null
      },
      { timeout },
    )
  },

  /** true if a chat input textarea is present inside the Shadow DOM. */
  async hasChatInput(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return root?.querySelector('[data-testid="chat-input"]') != null
    })
  },

  /** Texts of all suggested-question buttons inside the Shadow DOM. */
  async getQuestionTexts(): Promise<string[]> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const buttons = root?.querySelectorAll(
        '[data-testid="suggested-question-button"]',
      )
      return Array.from(buttons || []).map((b) => b.textContent || '')
    })
  },

  /** true if at least one suggested-question button is present. */
  async hasQuestionButtons(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      return (
        (root?.querySelectorAll('[data-testid="suggested-question-button"]')
          .length || 0) > 0
      )
    })
  },

  /** Wait until at least one suggested-question button is present. */
  async waitForQuestionButtons(timeout = 10000): Promise<void> {
    await page.waitForFunction(
      () => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        return (
          (root?.querySelectorAll('[data-testid="suggested-question-button"]')
            .length || 0) > 0
        )
      },
      { timeout },
    )
  },

  /**
   * Type a message into the widget chat input and click send.
   *
   * The widget's React `<textarea data-testid="chat-input">` is a *controlled*
   * component (`value={input}` + `onChange={(e) => setInput(e.target.value)}`,
   * `frontend/src/components/chat/chat-input.tsx`); the send button is
   * `disabled={!trimmed || isLoading}` where `trimmed` derives from that React
   * state. The widget mounts inside an **open** Shadow DOM
   * (`frontend/src/widget/main.tsx`: `attachShadow({ mode: 'open' })`).
   *
   * Synthetic value-set + `dispatchEvent('input')` does NOT reliably drive
   * React 18's controlled-component onChange inside a Shadow DOM (React's
   * synthetic event delegation does not consistently observe a manually
   * dispatched event there, even with `_valueTracker` reset). The reliable
   * approach is real keyboard input: focus the textarea and use
   * `page.keyboard.type()`, which emits genuine browser-level `keydown` /
   * `input` events targeting the focused element — React's onChange always
   * observes these. Keyboard events target the focused element regardless of
   * Shadow DOM boundaries.
   *
   * We focus via `page.evaluate` (Playwright `focus()` does not pierce shadow
   * roots), then type, wait for the send button to enable (React state
   * flushed), and press Enter (the input handles Enter via `onKeyDown` →
   * `handleSend`, so we avoid a separate shadow-DOM click entirely).
   */
  async sendMessage(text: string): Promise<void> {
    // (1) Focus the textarea inside the Shadow DOM.
    await page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const textarea = root?.querySelector(
        '[data-testid="chat-input"]',
      ) as HTMLTextAreaElement | null
      textarea?.focus()
    })

    // (2) Type the message — real keyboard input drives React's onChange.
    await page.keyboard.type(text, { delay: 5 })

    // (3) Wait for React to flush the state update and re-enable the send
    // button (its `disabled` DOM property clears once `trimmed` is non-empty).
    await page.waitForFunction(
      (msg) => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        const textarea = root?.querySelector(
          '[data-testid="chat-input"]',
        ) as HTMLTextAreaElement | null
        const sendBtn = root?.querySelector(
          '[data-testid="chat-send-button"]',
        ) as HTMLButtonElement | null
        return (
          !!textarea &&
          textarea.value === msg &&
          !!sendBtn &&
          !sendBtn.disabled
        )
      },
      text,
      { timeout: 10000 },
    )

    // (4) Click the send button inside the Shadow DOM. By now React has flushed
    // `setInput(msg)` and the button is enabled (guarded above), so the click
    // reaches `handleSend` with a non-empty `trimmed` and fires the request.
    // (Pressing Enter via `page.keyboard.press('Enter')` did NOT reliably
    // trigger the widget's React onKeyDown handler inside the Shadow DOM;
    // clicking the enabled button after the state-sync wait is reliable.)
    await page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const sendBtn = root?.querySelector(
        '[data-testid="chat-send-button"]',
      ) as HTMLElement | null
      sendBtn?.click()
    })
  },

  /**
   * Text of the last assistant message bubble inside the Shadow DOM.
   * Returns '' if no assistant message is present.
   */
  async getLastAssistantText(): Promise<string> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const msgs = root?.querySelectorAll(
        '[data-testid="message-item-assistant"]',
      )
      if (!msgs || msgs.length === 0) return ''
      return msgs[msgs.length - 1].textContent || ''
    })
  },

  /**
   * Wait for at least one assistant message to appear in the Shadow DOM.
   * The streaming cursor may vanish quickly on fast responses, so this waits
   * for the message bubble itself (always present once a response starts).
   */
  async waitForAssistantMessage(timeout = 60000): Promise<void> {
    await page.waitForFunction(
      () => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        return (
          (root?.querySelectorAll('[data-testid="message-item-assistant"]')
            .length || 0) > 0
        )
      },
      { timeout },
    )
  },

  /**
   * Wait for streaming to finish (no streaming cursor inside the Shadow DOM).
   */
  async waitForStreamingDone(timeout = 60000): Promise<void> {
    await page.waitForFunction(
      () => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        return (
          (root?.querySelectorAll('[data-testid="message-item-streaming"]')
            .length || 0) === 0
        )
      },
      { timeout },
    )
  },

  /**
   * true if the last assistant message is in the FAILED state (empty content,
   * not streaming). The widget renders this when the chat request fails —
   * e.g. an unconfigured channel returns HTTP 400 (`频道 {id} 未配置`), which
   * the stream hook surfaces as a failed assistant bubble
   * (`use-widget-chat-stream.ts`: `!response.ok` → setError + finishStreaming).
   *
   * Source: `frontend/src/components/chat/message-item.tsx`
   *   `isFailed = !isUser && !message.isStreaming && !message.content.trim()`
   * Failed render path shows the `responseFailed` message + a retry button.
   */
  async isLastAssistantFailed(): Promise<boolean> {
    return page.evaluate(() => {
      const host = document.getElementById('rwiki-chat-widget')
      const root = host?.shadowRoot
      const msgs = root?.querySelectorAll(
        '[data-testid="message-item-assistant"]',
      )
      if (!msgs || msgs.length === 0) return false
      const last = msgs[msgs.length - 1]
      // A failed bubble contains the retry button (message-retry-button),
      // which is only rendered on the `isFailed` path.
      return (
        last.querySelector('[data-testid="message-retry-button"]') !== null
      )
    })
  },

  /**
   * Wait for the last assistant message to enter the failed state (retry
   * button present). Used for the unconfigured-channel negative scenario.
   */
  async waitForAssistantFailed(timeout = 30000): Promise<void> {
    await page.waitForFunction(
      () => {
        const host = document.getElementById('rwiki-chat-widget')
        const root = host?.shadowRoot
        const msgs = root?.querySelectorAll(
          '[data-testid="message-item-assistant"]',
        )
        if (!msgs || msgs.length === 0) return false
        return (
          msgs[msgs.length - 1].querySelector(
            '[data-testid="message-retry-button"]',
          ) !== null
        )
      },
      { timeout },
    )
  },
})
