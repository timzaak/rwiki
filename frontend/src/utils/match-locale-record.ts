/**
 * Resolve a locale-keyed record to a value for the given locale.
 *
 * Matching (case-insensitive): (1) exact key, (2) longest key that is a
 * prefix of the locale. Returns undefined if no match.
 */
export function matchLocaleRecord<T>(
  record: Record<string, T>,
  locale: string
): T | undefined {
  const keys = Object.keys(record)
  if (keys.length === 0) return undefined

  // 1. Exact match (case-insensitive)
  const exactKey = keys.find((k) => k.toLowerCase() === locale.toLowerCase())
  if (exactKey) return record[exactKey]

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
  if (longestPrefixKey) return record[longestPrefixKey]

  return undefined
}
