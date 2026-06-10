import { useCallback } from 'react'
import Markdown from 'react-markdown'
import type { Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import hljs from 'highlight.js/lib/core'
import bash from 'highlight.js/lib/languages/bash'
import css from 'highlight.js/lib/languages/css'
import javascript from 'highlight.js/lib/languages/javascript'
import json from 'highlight.js/lib/languages/json'
import markdown from 'highlight.js/lib/languages/markdown'
import python from 'highlight.js/lib/languages/python'
import shell from 'highlight.js/lib/languages/shell'
import typescript from 'highlight.js/lib/languages/typescript'
import xml from 'highlight.js/lib/languages/xml'
import yaml from 'highlight.js/lib/languages/yaml'
import { AlertCircleIcon, BotIcon, RefreshCwIcon, UserIcon } from 'lucide-react'

import { FeedbackButtons } from './feedback-buttons'
import { useFeedback } from '@/hooks/use-feedback'
import { useChatStore } from '@/stores/chat-store'
import type { ChatMessage } from '@/stores/chat-store'
import { cn } from '@/lib/utils'

interface MessageItemProps {
  message: ChatMessage
  onRetry?: () => void
}

hljs.registerLanguage('bash', bash)
hljs.registerLanguage('css', css)
hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('json', json)
hljs.registerLanguage('markdown', markdown)
hljs.registerLanguage('python', python)
hljs.registerLanguage('shell', shell)
hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('yaml', yaml)

const languageAliases: Record<string, string> = {
  html: 'xml',
  js: 'javascript',
  jsx: 'javascript',
  md: 'markdown',
  sh: 'bash',
  ts: 'typescript',
  tsx: 'typescript',
}

const markdownComponents: Components = {
  code({ className, children, ...props }) {
    const match = /language-(\w+)/.exec(className ?? '')
    const requestedLanguage = match?.[1]
    const language = requestedLanguage
      ? languageAliases[requestedLanguage] ?? requestedLanguage
      : undefined

    if (!language || !hljs.getLanguage(language)) {
      return (
        <code className={className} {...props}>
          {children}
        </code>
      )
    }

    const highlighted = hljs.highlight(String(children).replace(/\n$/, ''), {
      language,
    }).value

    return (
      <code
        className={`hljs language-${language}`}
        dangerouslySetInnerHTML={{ __html: highlighted }}
        {...props}
      />
    )
  },
}

export function MessageItem({ message, onRetry }: MessageItemProps) {
  const isUser = message.role === 'user'
  const isFailed = !isUser && !message.isStreaming && !message.content.trim()

  const sessionId = useChatStore((s) => s.sessionId)
  const userMessage = useChatStore(
    useCallback(
      (s) => {
        if (isUser) return ''
        const idx = s.messages.findIndex((m) => m.id === message.id)
        for (let i = idx - 1; i >= 0; i--) {
          if (s.messages[i].role === 'user') return s.messages[i].content
        }
        return ''
      },
      [message.id, isUser],
    ),
  )

  const { feedback, submitFeedback, isSubmitting } = useFeedback({
    sessionId,
    messageId: message.id,
    userMessage,
    assistantMessage: message.content,
  })

  return (
    <div
      data-testid={`message-item-${message.role}`}
      className={cn(
        'flex gap-3 px-4 py-3 [animation:message-enter_0.3s_ease-out_both]',
        isUser ? 'flex-row-reverse' : 'flex-row',
      )}
    >
      <div
        className={cn(
          'flex size-8 shrink-0 items-center justify-center rounded-full shadow-sm',
          isUser
            ? 'bg-primary text-primary-foreground'
            : 'bg-secondary text-muted-foreground ring-1 ring-border/50',
        )}
      >
        {isUser ? (
          <UserIcon className="size-3.5" />
        ) : (
          <BotIcon className="size-3.5" />
        )}
      </div>

      <div
        className={cn(
          'max-w-[80%] rounded-2xl px-3.5 py-2.5 text-sm',
          isUser
            ? 'bg-primary text-primary-foreground rounded-br-md shadow-sm'
            : 'bg-card text-foreground rounded-bl-md shadow-sm ring-1 ring-border/40',
        )}
      >
        {isUser ? (
          <p className="whitespace-pre-wrap break-all">{message.content}</p>
        ) : isFailed ? (
          <div className="flex flex-col gap-2 py-0.5">
            <div className="flex items-start gap-2 text-muted-foreground">
              <AlertCircleIcon className="mt-0.5 size-3.5 shrink-0 text-destructive/70" />
              <span className="text-xs leading-relaxed">
                Response generation failed. Please try again.
              </span>
            </div>
            {onRetry && (
              <button
                type="button"
                data-testid="message-retry-button"
                className="inline-flex w-fit items-center gap-1 rounded-md bg-secondary/80 px-2 py-1 text-xs text-muted-foreground ring-1 ring-border/40 transition-colors hover:bg-secondary hover:text-foreground"
                onClick={onRetry}
              >
                <RefreshCwIcon className="size-3" />
                Retry
              </button>
            )}
          </div>
        ) : (
          <>
            <div className="prose prose-sm break-all max-w-none dark:prose-invert [&_a]:font-medium [&_a]:text-primary [&_a]:underline [&_a]:underline-offset-4 [&_a]:decoration-primary/30 hover:[&_a]:decoration-primary/60 [&_pre]:rounded-lg [&_pre]:bg-background [&_pre]:p-3 [&_pre]:ring-1 [&_pre]:ring-border/30 [&_pre]:overflow-x-auto [&_code:not(pre code)]:rounded [&_code:not(pre code)]:bg-secondary/60 [&_code:not(pre code)]:px-1.5 [&_code:not(pre code)]:py-0.5 [&_code:not(pre code)]:text-[0.85em]">
              <Markdown
                remarkPlugins={[remarkGfm]}
                components={markdownComponents}
              >
                {message.content}
              </Markdown>
            </div>

            {message.isStreaming && (
              <span
                data-testid="message-item-streaming"
                className="ml-1 inline-block size-1.5 animate-pulse rounded-full bg-current"
              />
            )}

            {message.error && (
              <span className="mt-1 inline-flex items-center gap-1 rounded-md bg-destructive/10 px-2 py-0.5 text-xs text-destructive ring-1 ring-destructive/20">
                {message.error}
              </span>
            )}
          </>
        )}

        {!isUser && !message.isStreaming && !isFailed && sessionId != null && (
          <FeedbackButtons
            feedback={feedback}
            onFeedback={submitFeedback}
            isSubmitting={isSubmitting}
          />
        )}
      </div>
    </div>
  )
}
