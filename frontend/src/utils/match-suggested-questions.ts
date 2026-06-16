import { matchLocaleRecord } from './match-locale-record'

/**
 * Match suggested questions to the current locale.
 *
 * - If input is an array, return it directly.
 * - If input is a Record, try (1) exact key match, (2) longest prefix match
 *   (both via {@link matchLocaleRecord}), (3) "default" key, then (4) return [].
 * - If input is undefined/null, return [].
 */
export function matchSuggestedQuestions(
  suggestedQuestions: string[] | Record<string, string[]> | undefined | null,
  locale: string
): string[] {
  if (suggestedQuestions == null) return []
  if (Array.isArray(suggestedQuestions)) return suggestedQuestions

  const matched = matchLocaleRecord(suggestedQuestions, locale)
  if (matched) return matched

  // "default" key fallback (case-insensitive)
  const defaultKey = Object.keys(suggestedQuestions).find(
    (k) => k.toLowerCase() === 'default'
  )
  return defaultKey ? suggestedQuestions[defaultKey] ?? [] : []
}
