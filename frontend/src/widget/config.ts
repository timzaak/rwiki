import type { Locale, WidgetMessages } from '@/components/chat/messages';
import { resolveLocale } from '@/components/chat/messages';

export interface WidgetConfig {
  apiUrl: string;
  /**
   * 频道标识；可传单个字符串或字符串数组。
   *
   * 归一化后（`validateWidgetConfig`）始终变为去重的 `string[]`。
   * 多频道时按"跨频道并集检索"语义命中任一频道的已发布文档。
   */
  channelId: string | string[];
  primaryColor?: string;
  title?: string;
  position?: 'left' | 'right';
  welcomeMessage?: string;
  suggestedQuestions?: string[] | Record<string, string[]>;
  locale?: string;
  messages?: Partial<WidgetMessages>;
}

export const WIDGET_DEFAULTS = {
  primaryColor: '#3b82f6',
  position: 'right' as const,
};

/**
 * Normalize channelId input (string | string[]) into a deduped, order-preserving,
 * non-empty string[].
 *
 * - 每个元素 trim；过滤掉空串
 * - 去重（保留首次出现顺序）
 * - 结果为空数组 → 返回 null（调用方据此报 "channelId is required"）
 * - 非法类型（number、object 等）→ 返回 null
 */
function normalizeChannelIds(input: unknown): string[] | null {
  let ids: unknown[]
  if (Array.isArray(input)) {
    ids = input
  } else if (typeof input === 'string') {
    ids = [input]
  } else {
    return null
  }

  const seen = new Set<string>()
  const result: string[] = []
  for (const item of ids) {
    if (typeof item !== 'string') return null
    const trimmed = item.trim()
    if (trimmed === '') continue
    if (!seen.has(trimmed)) {
      seen.add(trimmed)
      result.push(trimmed)
    }
  }
  return result.length > 0 ? result : null
}

export interface ValidatedWidgetConfig
  extends Required<Omit<WidgetConfig, 'welcomeMessage' | 'suggestedQuestions' | 'title' | 'locale' | 'messages' | 'channelId'>> {
  /** 归一化后的频道列表：去重、保序、非空元素。 */
  channelId: string[];
  title?: string;
  welcomeMessage?: string;
  suggestedQuestions?: string[] | Record<string, string[]>;
  locale: Locale;
  messages?: Partial<WidgetMessages>;
}

export function validateWidgetConfig(config: Partial<WidgetConfig>): ValidatedWidgetConfig | null {
  if (!config.apiUrl) {
    console.error('[RWikiChat] apiUrl is required');
    return null;
  }

  if (!/^https?:\/\//i.test(config.apiUrl)) {
    console.error('[RWikiChat] apiUrl must start with http:// or https://');
    return null;
  }

  // channelId: accept string | string[]; normalize to a deduped, ordered, non-empty string[].
  const channelId = normalizeChannelIds(config.channelId);
  if (channelId === null) {
    console.error('[RWikiChat] channelId is required');
    return null;
  }

  if (config.primaryColor && !/^#[0-9a-fA-F]{6}$/.test(config.primaryColor)) {
    console.error('[RWikiChat] primaryColor must be a 6-digit hex color');
    return null;
  }

  if (config.position && !['left', 'right'].includes(config.position)) {
    console.error('[RWikiChat] position must be "left" or "right"');
    return null;
  }

  if (config.suggestedQuestions != null) {
    const sq = config.suggestedQuestions;
    const isArray = Array.isArray(sq);
    const isRecord = !isArray && typeof sq === 'object' && Object.values(sq).every((v) => Array.isArray(v));
    if (!isArray && !isRecord) {
      console.error('[RWikiChat] suggestedQuestions must be a string[] or Record<string, string[]>');
      return null;
    }
  }

  if (config.locale !== undefined && typeof config.locale !== 'string') {
    console.error('[RWikiChat] locale must be a string');
    return null;
  }

  if (config.locale !== undefined && config.locale.trim() === '') {
    console.error('[RWikiChat] locale must be a non-empty string');
    return null;
  }

  const apiUrl = config.apiUrl.replace(/\/+$/, '');

  const validated: ValidatedWidgetConfig = {
    apiUrl,
    channelId,
    primaryColor: config.primaryColor ?? WIDGET_DEFAULTS.primaryColor,
    position: config.position ?? WIDGET_DEFAULTS.position,
    locale: resolveLocale(config.locale ?? navigator.language),
    ...(config.title && { title: config.title }),
    ...(config.welcomeMessage && { welcomeMessage: config.welcomeMessage }),
    ...(config.suggestedQuestions && { suggestedQuestions: config.suggestedQuestions }),
    ...(config.messages && { messages: config.messages }),
  };

  return validated;
}
