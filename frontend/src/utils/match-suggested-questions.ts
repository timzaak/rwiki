/**
 * Match suggested questions to the current locale.
 *
 * - If input is an array, return it directly.
 * - If input is a Record, try (1) exact key match, (2) longest prefix match,
 *   (3) "default" key, then (4) return [].
 * - If input is undefined/null, return [].
 */
export function matchSuggestedQuestions(
  suggestedQuestions: string[] | Record<string, string[]> | undefined | null,
  locale: string
): string[] {
  if (suggestedQuestions == null) return []
  if (Array.isArray(suggestedQuestions)) return suggestedQuestions

  const keys = Object.keys(suggestedQuestions)
  if (keys.length === 0) return []

  // 1. Exact match (case-insensitive)
  const exactKey = keys.find((k) => k.toLowerCase() === locale.toLowerCase())
  if (exactKey) return suggestedQuestions[exactKey] ?? []

  // 2. Longest prefix match: find keys that are a prefix of locale
  const localeLower = locale.toLowerCase()
  let longestPrefixKey: string | null = null
  let longestPrefixLen = 0
  for (const key of keys) {
    const keyLower = key.toLowerCase()
    if (localeLower.startsWith(keyLower) && keyLower.length > longestPrefixLen) {
      longestPrefixKey = key
      longestPrefixLen = keyLower.length
    }
  }
  if (longestPrefixKey) return suggestedQuestions[longestPrefixKey] ?? []

  // 3. "default" key
  const defaultKey = keys.find((k) => k.toLowerCase() === 'default')
  if (defaultKey) return suggestedQuestions[defaultKey] ?? []

  // 4. Fallback
  return []
}
