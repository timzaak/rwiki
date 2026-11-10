import { Component, type ReactNode } from 'react';
import { useCallback, useEffect } from 'react';

import { ChatStreamProvider } from '@/components/chat/chat-stream-context';
import { FloatingButton } from '@/components/chat/floating-button';
import { ChatModal } from '@/components/chat/chat-modal';
import { ChatSidePanel } from '@/components/chat/chat-side-panel';
import { ChannelIdProvider } from '@/components/chat/channel-id-context';
import { WidgetI18nProvider } from '@/components/chat/widget-i18n';
import { WIDGET_MESSAGES, resolveWidgetMessages } from '@/components/chat/messages';
import { FeedbackSubmitFnContext } from '@/hooks/feedback-context';
import { client } from '@/lib/api-generated/client.gen';
import { submitFeedback } from '@/lib/api-generated/sdk.gen';
import type { FeedbackRequest } from '@/lib/api-generated/types.gen';
import { useChatStore } from '@/stores/chat-store';
import { useWidgetChatStream } from '@/widget/use-widget-chat-stream';
import { useWidgetSuggestions } from '@/widget/use-widget-suggestions';
import type { ValidatedWidgetConfig } from '@/widget/config';

interface WidgetAppProps {
  config: ValidatedWidgetConfig;
}

function WidgetAppContent({ config }: WidgetAppProps) {
  const streamValue = useWidgetChatStream(config.apiUrl, config.channelId);
  const suggestedQuestions = useWidgetSuggestions(config.apiUrl, config.locale, config.channelId, config.suggestedQuestions);
  const t = resolveWidgetMessages(config.locale, config.messages);
  const effectiveTitle = config.title ?? t.titleDefault;

  const feedbackSubmitFn = useCallback(
    async (body: FeedbackRequest) => {
      client.setConfig({ baseUrl: config.apiUrl })
      await submitFeedback<true>({ body: { ...body, channelId: config.channelId }, throwOnError: true })
    },
    [config.apiUrl, config.channelId],
  )

  // 消费全局 API RWikiChat.ask() 投递的问题（US-INTG-017）：React 树外只能写 store，
  // 发送逻辑在树内。先清空再发送，发送失败时由 chat 自身的错误态呈现，不重试。
  // 发送中的回答按既有隐式中断语义收尾（US-CORE-039/040）。
  const pendingQuestion = useChatStore((s) => s.pendingQuestion)
  const { sendMessage } = streamValue
  useEffect(() => {
    if (pendingQuestion == null) return
    useChatStore.getState().setPendingQuestion(null)
    void sendMessage(pendingQuestion)
  }, [pendingQuestion, sendMessage])

  return (
    <WidgetI18nProvider messages={t}>
      <ChannelIdProvider channelId={config.channelId}>
        <FeedbackSubmitFnContext.Provider value={feedbackSubmitFn}>
          <ChatStreamProvider value={streamValue}>
            <FloatingButton position={config.position} />
            {config.displayMode === 'sidePanel' ? (
              <ChatSidePanel
                title={effectiveTitle}
                welcomeMessage={config.welcomeMessage}
                suggestedQuestions={suggestedQuestions}
              />
            ) : (
              <ChatModal
                title={effectiveTitle}
                position={config.position}
                welcomeMessage={config.welcomeMessage}
                suggestedQuestions={suggestedQuestions}
              />
            )}
          </ChatStreamProvider>
        </FeedbackSubmitFnContext.Provider>
      </ChannelIdProvider>
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
