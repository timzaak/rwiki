import { useEffect, useState } from 'react'

import type { Locale } from '@/components/chat/messages'

const cache = new Map<string, string[]>()

export function useWidgetSuggestions(
  apiUrl: string,
  locale: Locale,
  siteId: string,
  // Kept for call-site compatibility; intentionally ignored (site-strict).
  _fallback?: string[] | Record<string, string[]>,
): string[] {
  const cacheKey = `${siteId}:${locale}`
  const [questions, setQuestions] = useState<string[]>(() => cache.get(cacheKey) ?? [])

  useEffect(() => {
    if (cache.has(cacheKey)) {
      setQuestions(cache.get(cacheKey)!)
      return
    }

    let cancelled = false
    fetch(`${apiUrl}/api/chat/suggestions?locale=${encodeURIComponent(locale)}&siteId=${encodeURIComponent(siteId)}`)
      .then((r) => r.json())
      .then((data: { questions?: string[] }) => {
        if (cancelled) return
        const next = data.questions ?? []
        cache.set(cacheKey, next)
        setQuestions(next)
      })
      .catch(() => {})

    return () => {
      cancelled = true
    }
  }, [apiUrl, locale, siteId, cacheKey])

  return questions
}
