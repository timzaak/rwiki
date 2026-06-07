import { useEffect, useState } from 'react'

import { matchSuggestedQuestions } from '@/utils/match-suggested-questions'

const cache = new Map<string, string[]>()

export function useWidgetSuggestions(
  apiUrl: string,
  fallback?: string[] | Record<string, string[]>,
): string[] {
  const [questions, setQuestions] = useState<string[]>(() => {
    const cached = cache.get(navigator.language)
    return cached ?? matchSuggestedQuestions(fallback, navigator.language)
  })

  useEffect(() => {
    const locale = navigator.language
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
  }, [apiUrl])

  return questions
}
