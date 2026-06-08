import { Component, type ReactNode } from 'react';
import { useCallback } from 'react';

import { ChatStreamProvider } from '@/components/chat/chat-stream-context';
import { FloatingButton } from '@/components/chat/floating-button';
import { ChatModal } from '@/components/chat/chat-modal';
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
  const streamValue = useWidgetChatStream(config.apiUrl);
  const suggestedQuestions = useWidgetSuggestions(config.apiUrl, config.suggestedQuestions);

  const feedbackSubmitFn = useCallback(
    async (body: FeedbackRequest) => {
      client.setConfig({ baseUrl: config.apiUrl })
      await submitFeedback<true>({ body, throwOnError: true })
    },
    [config.apiUrl],
  )

  return (
    <FeedbackSubmitFnContext.Provider value={feedbackSubmitFn}>
      <ChatStreamProvider value={streamValue}>
        <FloatingButton position={config.position} />
        <ChatModal
          title={config.title}
          position={config.position}
          welcomeMessage={config.welcomeMessage}
          suggestedQuestions={suggestedQuestions}
        />
      </ChatStreamProvider>
    </FeedbackSubmitFnContext.Provider>
  );
}

class WidgetErrorBoundary extends Component<
  { children: ReactNode },
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
          Widget encountered an error. Call RWikiChat.destroy() and try again.
        </div>
      );
    }
    return this.props.children;
  }
}

export function WidgetApp({ config }: WidgetAppProps) {
  return (
    <WidgetErrorBoundary>
      <WidgetAppContent config={config} />
    </WidgetErrorBoundary>
  );
}
