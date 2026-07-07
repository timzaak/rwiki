import { useEffect, useState } from 'react'

import type { Locale } from '@/components/chat/messages'

const cache = new Map<string, string[]>()

export function useWidgetSuggestions(
  apiUrl: string,
  locale: Locale,
  channelId: string,
  // Kept for call-site compatibility; intentionally ignored (channel-strict).
  _fallback?: string[] | Record<string, string[]>,
): string[] {
  const cacheKey = `${channelId}:${locale}`
  const [questions, setQuestions] = useState<string[]>(() => cache.get(cacheKey) ?? [])

  useEffect(() => {
    if (cache.has(cacheKey)) {
      setQuestions(cache.get(cacheKey)!)
      return
    }

    let cancelled = false
    fetch(`${apiUrl}/api/chat/suggestions?locale=${encodeURIComponent(locale)}&channelId=${encodeURIComponent(channelId)}`)
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
  }, [apiUrl, locale, channelId, cacheKey])

  return questions
}
