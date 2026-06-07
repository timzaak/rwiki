import { Component, type ReactNode } from 'react';

import { ChatStreamProvider } from '@/components/chat/chat-stream-context';
import { FloatingButton } from '@/components/chat/floating-button';
import { ChatModal } from '@/components/chat/chat-modal';
import { useWidgetChatStream } from '@/widget/use-widget-chat-stream';
import { matchSuggestedQuestions } from '@/utils/match-suggested-questions';
import type { ValidatedWidgetConfig } from '@/widget/config';

interface WidgetAppProps {
  config: ValidatedWidgetConfig;
}

function WidgetAppContent({ config }: WidgetAppProps) {
  const streamValue = useWidgetChatStream(config.apiUrl);
  const suggestedQuestions = matchSuggestedQuestions(config.suggestedQuestions, navigator.language);

  return (
    <ChatStreamProvider value={streamValue}>
      <FloatingButton position={config.position} />
      <ChatModal
        title={config.title}
        position={config.position}
        welcomeMessage={config.welcomeMessage}
        suggestedQuestions={suggestedQuestions}
      />
    </ChatStreamProvider>
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
