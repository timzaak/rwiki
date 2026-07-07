import type { Locale, WidgetMessages } from '@/components/chat/messages';
import { resolveLocale } from '@/components/chat/messages';

export interface WidgetConfig {
  apiUrl: string;
  channelId: string;
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

export interface ValidatedWidgetConfig
  extends Required<Omit<WidgetConfig, 'welcomeMessage' | 'suggestedQuestions' | 'title' | 'locale' | 'messages'>> {
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

  if (!config.channelId || typeof config.channelId !== 'string' || config.channelId.trim() === '') {
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
    channelId: config.channelId.trim(),
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
