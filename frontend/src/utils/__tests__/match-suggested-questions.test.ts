import { describe, it, expect } from 'vitest'
import { matchSuggestedQuestions } from '@/utils/match-suggested-questions'

function makeRecord(overrides?: Record<string, string[]>): Record<string, string[]> {
  return {
    default: ['default q1', 'default q2'],
    en: ['en q1', 'en q2'],
    zh: ['zh q1', 'zh q2'],
    'zh-CN': ['zh-CN q1'],
    ...overrides,
  }
}

describe('matchSuggestedQuestions', () => {
  describe('array passthrough', () => {
    it('returns the same array when input is a non-empty array', () => {
      const input = ['q1', 'q2', 'q3']
      expect(matchSuggestedQuestions(input, 'en')).toBe(input)
    })

    it('returns an empty array when input is an empty array', () => {
      expect(matchSuggestedQuestions([], 'en')).toEqual([])
    })
  })

  describe('undefined/null input', () => {
    it.each([
      { input: undefined, label: 'undefined' },
      { input: null, label: 'null' },
    ])('returns empty array for $label', ({ input }) => {
      expect(matchSuggestedQuestions(input, 'en')).toEqual([])
    })
  })

  describe('exact locale match', () => {
    it.each([
      { locale: 'zh-CN', description: 'identical case' },
      { locale: 'zh-cn', description: 'lowercase' },
      { locale: 'ZH-CN', description: 'uppercase' },
      { locale: 'Zh-Cn', description: 'mixed case' },
    ])(
      'matches key "zh-CN" with locale "$locale" ($description)',
      ({ locale }) => {
        expect(matchSuggestedQuestions(makeRecord(), locale)).toEqual([
          'zh-CN q1',
        ])
      }
    )

    it('exact match takes priority over prefix match', () => {
      const record = makeRecord()
      // "zh-CN" key exists in base record; locale "zh-CN" should hit exact,
      // not fall through to the "zh" prefix.
      expect(matchSuggestedQuestions(record, 'zh-CN')).toEqual(['zh-CN q1'])
    })
  })

  describe('longest prefix match', () => {
    it('falls back to prefix when no exact key exists', () => {
      const record: Record<string, string[]> = { zh: ['zh val'] }
      expect(matchSuggestedQuestions(record, 'zh-TW')).toEqual(['zh val'])
    })

    it('picks the longest matching prefix', () => {
      const record: Record<string, string[]> = {
        zh: ['zh val'],
        'zh-CN': ['zh-CN val'],
      }
      // "zh-CN-variant" has no exact match, but "zh-CN" is a longer prefix than "zh"
      expect(matchSuggestedQuestions(record, 'zh-CN-variant')).toEqual([
        'zh-CN val',
      ])
    })
  })

  describe('default key fallback', () => {
    it('returns default value when no exact or prefix match exists', () => {
      const record = makeRecord()
      // "ja" has no exact or prefix match among keys
      expect(matchSuggestedQuestions(record, 'ja')).toEqual([
        'default q1',
        'default q2',
      ])
    })

    it('recognizes "DEFAULT" as default key (case-insensitive)', () => {
      const record: Record<string, string[]> = {
        DEFAULT: ['fallback val'],
        en: ['en val'],
      }
      expect(matchSuggestedQuestions(record, 'fr')).toEqual(['fallback val'])
    })
  })

  describe('no match', () => {
    it('returns empty array when no keys match and no default exists', () => {
      const record: Record<string, string[]> = {
        en: ['en val'],
        zh: ['zh val'],
      }
      expect(matchSuggestedQuestions(record, 'ja')).toEqual([])
    })

    it('returns empty array for an empty record', () => {
      expect(matchSuggestedQuestions({}, 'en')).toEqual([])
    })
  })
})
