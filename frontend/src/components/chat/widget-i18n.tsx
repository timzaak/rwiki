import { createContext, useContext, type ReactNode } from 'react'

import { WIDGET_MESSAGES, type WidgetMessages } from './messages'

const WidgetI18nContext = createContext<WidgetMessages>(WIDGET_MESSAGES.en)

export function WidgetI18nProvider({
  messages,
  children,
}: {
  messages: WidgetMessages
  children: ReactNode
}) {
  return (
    <WidgetI18nContext.Provider value={messages}>
      {children}
    </WidgetI18nContext.Provider>
  )
}

// eslint-disable-next-line react-refresh/only-export-components
export function useWidgetI18n(): WidgetMessages {
  return useContext(WidgetI18nContext)
}
