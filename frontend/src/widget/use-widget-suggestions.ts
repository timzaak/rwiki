import { useEffect, useState } from 'react'

import type { Locale } from '@/components/chat/messages'
import { matchSuggestedQuestions } from '@/utils/match-suggested-questions'

const cache = new Map<string, string[]>()

export function useWidgetSuggestions(
  apiUrl: string,
  locale: Locale,
  fallback?: string[] | Record<string, string[]>,
): string[] {
  const [questions, setQuestions] = useState<string[]>(() => {
    const cached = cache.get(locale)
    return cached ?? matchSuggestedQuestions(fallback, locale)
  })

  useEffect(() => {
    if (cache.has(locale)) {
      setQuestions(cache.get(locale)!)
      return
    }

    let cancelled = false
    fetch(`${apiUrl}/api/chat/suggestions?locale=${encodeURIComponent(locale)}`)
      .then((r) => r.json())
      .then((data: { questions?: string[] }) => {
        if (!cancelled && data.questions?.length) {
          cache.set(locale, data.questions)
          setQuestions(data.questions)
        }
      })
      .catch(() => {})

    return () => {
      cancelled = true
    }
  }, [apiUrl, locale])

  return questions
}
