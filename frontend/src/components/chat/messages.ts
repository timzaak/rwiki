import { matchLocaleRecord } from '@/utils/match-locale-record'

export type Locale = 'en' | 'zh-CN'

export interface WidgetMessages {
  /** Default chat title used when the host does not pass `title`. */
  titleDefault: string
  inputPlaceholder: string
  responseFailed: string
  responseInterrupted: string
  retry: string
  a11yOpen: string
  a11yClose: string
  a11yClear: string
  a11yLike: string
  a11yDislike: string
  errorBoundary: string
}

export const WIDGET_MESSAGES: Record<Locale, WidgetMessages> = {
  en: {
    titleDefault: 'Rwiki Chat',
    inputPlaceholder: 'Ask anything...',
    responseFailed: 'Response generation failed. Please try again.',
    responseInterrupted:
      'The response was interrupted; the content above may be incomplete.',
    retry: 'Retry',
    a11yOpen: 'Open chat assistant',
    a11yClose: 'Close chat modal',
    a11yClear: 'Clear current conversation',
    a11yLike: 'Like',
    a11yDislike: 'Dislike',
    errorBoundary:
      'Widget encountered an error. Call RWikiChat.destroy() and try again.',
  },
  'zh-CN': {
    titleDefault: 'Rwiki 助手',
    inputPlaceholder: '随便问点什么…',
    responseFailed: '回复生成失败,请重试。',
    responseInterrupted: '回答被中断,以上内容可能不完整。',
    retry: '重试',
    a11yOpen: '打开聊天助手',
    a11yClose: '关闭聊天窗口',
    a11yClear: '清空当前对话',
    a11yLike: '赞',
    a11yDislike: '踩',
    errorBoundary:
      '小组件发生错误,请调用 RWikiChat.destroy() 后重试。',
  },
}

export const SUPPORTED_LOCALES = Object.keys(WIDGET_MESSAGES) as Locale[]

/**
 * Resolve a raw locale tag to one of the supported locales.
 * Priority:
 *   1. Any Chinese variant (`zh`, `zh-Hans`, `zh-TW`, `zh-HK`, …) → `zh-CN`.
 *      (We ship a single Chinese set; English fallback would hurt the project's
 *      primary audience.)
 *   2. `matchLocaleRecord` over WIDGET_MESSAGES keys (exact → longest prefix).
 *   3. Fall back to the base locale `en`.
 */
export function resolveLocale(rawLocale: string | undefined): Locale {
  const raw = (rawLocale ?? '').trim().toLowerCase() || 'en'
  if (raw === 'zh' || raw.startsWith('zh-')) {
    return 'zh-CN'
  }
  const matched = matchLocaleRecord(WIDGET_MESSAGES, raw)
  if (matched) {
    // find the key whose value matched (supported locale)
    const key = SUPPORTED_LOCALES.find((l) => WIDGET_MESSAGES[l] === matched)
    if (key) return key
  }
  return 'en'
}

/** Merge host overrides on top of the resolved locale's messages. */
export function resolveWidgetMessages(
  locale: Locale,
  overrides?: Partial<WidgetMessages>
): WidgetMessages {
  return { ...WIDGET_MESSAGES[locale] ?? WIDGET_MESSAGES.en, ...overrides }
}
