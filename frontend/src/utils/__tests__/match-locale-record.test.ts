import { describe, it, expect } from 'vitest'

import { matchLocaleRecord } from '@/utils/match-locale-record'

describe('matchLocaleRecord', () => {
  it('returns undefined for an empty record', () => {
    expect(matchLocaleRecord({}, 'en')).toBeUndefined()
  })

  it('matches a key exactly (case-insensitive)', () => {
    const record = { 'zh-CN': 'a', en: 'b' }
    expect(matchLocaleRecord(record, 'zh-CN')).toBe('a')
    expect(matchLocaleRecord(record, 'zh-cn')).toBe('a')
    expect(matchLocaleRecord(record, 'ZH-CN')).toBe('a')
    expect(matchLocaleRecord(record, 'Zh-Cn')).toBe('a')
  })

  it('falls back to the longest matching prefix when there is no exact match', () => {
    const record = { zh: 'short', 'zh-CN': 'long' }
    // "zh-CN-variant" matches both "zh" and "zh-CN" as prefixes; longest wins
    expect(matchLocaleRecord(record, 'zh-CN-variant')).toBe('long')
  })

  it('prefers a shorter prefix over nothing when no longer prefix matches', () => {
    const record = { zh: 'short' }
    expect(matchLocaleRecord(record, 'zh-TW')).toBe('short')
  })

  it('returns undefined when nothing matches', () => {
    const record = { en: 'x', 'zh-CN': 'y' }
    expect(matchLocaleRecord(record, 'fr')).toBeUndefined()
    expect(matchLocaleRecord(record, 'ja-JP')).toBeUndefined()
  })
})
