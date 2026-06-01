export interface WidgetConfig {
  apiUrl: string;
  primaryColor?: string;
  title?: string;
  position?: 'left' | 'right';
  welcomeMessage?: string;
}

export const WIDGET_DEFAULTS = {
  primaryColor: '#3b82f6',
  title: 'Chat Assistant',
  position: 'right' as const,
};

export interface ValidatedWidgetConfig extends Required<Omit<WidgetConfig, 'welcomeMessage'>> {
  welcomeMessage?: string;
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

  if (config.primaryColor && !/^#[0-9a-fA-F]{6}$/.test(config.primaryColor)) {
    console.error('[RWikiChat] primaryColor must be a 6-digit hex color');
    return null;
  }

  if (config.position && !['left', 'right'].includes(config.position)) {
    console.error('[RWikiChat] position must be "left" or "right"');
    return null;
  }

  const apiUrl = config.apiUrl.replace(/\/+$/, '');

  return {
    apiUrl,
    primaryColor: config.primaryColor ?? WIDGET_DEFAULTS.primaryColor,
    title: config.title ?? WIDGET_DEFAULTS.title,
    position: config.position ?? WIDGET_DEFAULTS.position,
    ...(config.welcomeMessage && { welcomeMessage: config.welcomeMessage }),
  };
}
