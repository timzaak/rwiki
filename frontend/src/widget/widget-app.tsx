import { Component, type ReactNode } from 'react';
import { useCallback } from 'react';

import { ChatStreamProvider } from '@/components/chat/chat-stream-context';
import { FloatingButton } from '@/components/chat/floating-button';
import { ChatModal } from '@/components/chat/chat-modal';
import { SiteIdProvider } from '@/components/chat/site-id-context';
import { WidgetI18nProvider } from '@/components/chat/widget-i18n';
import { WIDGET_MESSAGES, resolveWidgetMessages } from '@/components/chat/messages';
import { FeedbackSubmitFnContext } from '@/hooks/feedback-context';
import { client } from '@/lib/api-generated/client.gen';
import { submitFeedback } from '@/lib/api-generated/sdk.gen';
import type { FeedbackRequest } from '@/lib/api-generated/types.gen';
import { useWidgetChatStream } from '@/widget/use-widget-chat-stream';
import { useWidgetSuggestions } from '@/widget/use-widget-suggestions';
import type { ValidatedWidgetConfig } from '@/widget/config';

interface WidgetAppProps {
  config: ValidatedWidgetConfig;
}

function WidgetAppContent({ config }: WidgetAppProps) {
  const streamValue = useWidgetChatStream(config.apiUrl, config.siteId);
  const suggestedQuestions = useWidgetSuggestions(config.apiUrl, config.locale, config.siteId, config.suggestedQuestions);
  const t = resolveWidgetMessages(config.locale, config.messages);
  const effectiveTitle = config.title ?? t.titleDefault;

  const feedbackSubmitFn = useCallback(
    async (body: FeedbackRequest) => {
      client.setConfig({ baseUrl: config.apiUrl })
      await submitFeedback<true>({ body: { ...body, siteId: config.siteId }, throwOnError: true })
    },
    [config.apiUrl, config.siteId],
  )

  return (
    <WidgetI18nProvider messages={t}>
      <SiteIdProvider siteId={config.siteId}>
        <FeedbackSubmitFnContext.Provider value={feedbackSubmitFn}>
          <ChatStreamProvider value={streamValue}>
            <FloatingButton position={config.position} />
            <ChatModal
              title={effectiveTitle}
              position={config.position}
              welcomeMessage={config.welcomeMessage}
              suggestedQuestions={suggestedQuestions}
            />
          </ChatStreamProvider>
        </FeedbackSubmitFnContext.Provider>
      </SiteIdProvider>
    </WidgetI18nProvider>
  );
}

class WidgetErrorBoundary extends Component<
  { children: ReactNode; errorText?: string },
  { hasError: boolean }
> {
  state = { hasError: false };

  static getDerivedStateFromError() {
    return { hasError: true };
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          padding: '16px',
          color: '#dc2626',
          fontSize: '14px',
          fontFamily: 'sans-serif',
        }}>
          {this.props.errorText ?? WIDGET_MESSAGES.en.errorBoundary}
        </div>
      );
    }
    return this.props.children;
  }
}

export function WidgetApp({ config }: WidgetAppProps) {
  const errorText = resolveWidgetMessages(config.locale, config.messages).errorBoundary;
  return (
    <WidgetErrorBoundary errorText={errorText}>
      <WidgetAppContent config={config} />
    </WidgetErrorBoundary>
  );
}
