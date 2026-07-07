import { describe, it, expect, vi, beforeEach } from 'vitest'

import { validateWidgetConfig } from '../config'

describe('validateWidgetConfig', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  it('returns null and logs error when apiUrl is missing (checked before channelId)', () => {
    // apiUrl is checked BEFORE channelId: when BOTH are missing, the apiUrl error
    // is the one that fires and the function returns immediately — channelId error
    // never fires. This makes the impl's ordering observable.
    const result = validateWidgetConfig({})
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledTimes(1)
    expect(console.error).toHaveBeenNthCalledWith(1, '[RWikiChat] apiUrl is required')
  })

  it('returns null when apiUrl does not start with http:// or https://', () => {
    const result = validateWidgetConfig({ apiUrl: 'ftp://example.com', channelId: 'help-center' })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] apiUrl must start with http:// or https://',
    )
  })

  it('strips trailing slashes from apiUrl', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com/', channelId: 'help-center' })
    expect(result).not.toBeNull()
    expect(result!.apiUrl).toBe('http://example.com')
  })

  it('returns null when primaryColor is not a valid hex color', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      primaryColor: 'red',
    })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] primaryColor must be a 6-digit hex color',
    )
  })

  it('preserves valid primaryColor in result', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      primaryColor: '#ff5500',
    })
    expect(result).not.toBeNull()
    expect(result!.primaryColor).toBe('#ff5500')
  })

  it('returns null when position is invalid', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      position: 'top' as any,
    })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] position must be "left" or "right"',
    )
  })

  it('returns ValidatedWidgetConfig with defaults for valid full config', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      primaryColor: '#ff5500',
      title: 'My Bot',
      position: 'left',
      welcomeMessage: 'Hi!',
    })
    expect(result).toEqual({
      apiUrl: 'http://example.com',
      channelId: ['help-center'],
      primaryColor: '#ff5500',
      title: 'My Bot',
      position: 'left',
      locale: 'en',
      welcomeMessage: 'Hi!',
    })
  })

  it('applies defaults when only apiUrl and channelId are provided', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com', channelId: 'help-center' })
    expect(result).toEqual({
      apiUrl: 'http://example.com',
      channelId: ['help-center'],
      primaryColor: '#3b82f6',
      position: 'right',
      locale: 'en',
    })
  })

  it('does not include a title key when no title is provided (resolved at render time)', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com', channelId: 'help-center' })
    expect(result).not.toHaveProperty('title')
  })

  it('resolves locale from navigator.language by default (en-US -> en)', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com', channelId: 'help-center' })
    expect(result!.locale).toBe('en')
  })

  it('preserves an explicit supported locale', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      locale: 'zh-CN',
    })
    expect(result!.locale).toBe('zh-CN')
  })

  it('resolves an explicit locale prefix to a supported locale (zh-CN-Hans -> zh-CN)', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      locale: 'zh-CN-Hans',
    })
    expect(result!.locale).toBe('zh-CN')
  })

  it('resolves Chinese variants (zh-Hans / zh-TW / zh-HK / zh) to zh-CN', () => {
    for (const locale of ['zh-Hans', 'zh-TW', 'zh-HK', 'zh']) {
      const result = validateWidgetConfig({
        apiUrl: 'http://example.com',
        channelId: 'help-center',
        locale,
      })
      expect(result!.locale).toBe('zh-CN')
    }
  })

  it('falls back to en for an unsupported locale', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      locale: 'fr',
    })
    expect(result!.locale).toBe('en')
  })

  it('passes messages overrides through to the validated config', () => {
    const overrides = { inputPlaceholder: 'Custom' }
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      messages: overrides,
    })
    expect(result).toHaveProperty('messages', overrides)
  })

  it('returns null when locale is an empty string', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      locale: '',
    })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] locale must be a non-empty string',
    )
  })

  it('omits welcomeMessage when undefined', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com', channelId: 'help-center' })
    expect(result).not.toHaveProperty('welcomeMessage')
  })

  it('includes welcomeMessage when provided', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      channelId: 'help-center',
      welcomeMessage: 'How can I help?',
    })
    expect(result).toHaveProperty('welcomeMessage', 'How can I help?')
  })

  // ── channelId validation (FE-D01: channelId is required + normalized to string[]) ──
  describe('channelId validation (required + normalized to string[])', () => {
    // Boundary table: every invalid shape → null + console.error mentions
    // 'channelId is required'. Uses a VALID apiUrl so we reach the channelId check.
    it.each([
      ['missing (undefined)', { apiUrl: 'http://example.com' }],
      ['empty string', { apiUrl: 'http://example.com', channelId: '' }],
      ['whitespace-only', { apiUrl: 'http://example.com', channelId: '   ' }],
      ['empty array', { apiUrl: 'http://example.com', channelId: [] }],
      ['array of empty strings', { apiUrl: 'http://example.com', channelId: ['', '  '] }],
      ['non-string (number)', { apiUrl: 'http://example.com', channelId: 123 as any }],
    ])('returns null and logs channelId error when channelId is %s', (_label, config) => {
      const result = validateWidgetConfig(config)
      expect(result).toBeNull()
      expect(console.error).toHaveBeenCalledWith('[RWikiChat] channelId is required')
    })

    it('normalizes a single string channelId into a one-element array (trimmed)', () => {
      const result = validateWidgetConfig({
        apiUrl: 'http://example.com',
        channelId: '  help-center  ',
      })
      expect(result).not.toBeNull()
      expect(result!.channelId).toEqual(['help-center'])
    })

    it('includes the normalized channelId[] alongside a valid apiUrl in the result', () => {
      const result = validateWidgetConfig({
        apiUrl: 'http://example.com/',
        channelId: 'help-center',
      })
      expect(result).not.toBeNull()
      expect(result!.channelId).toEqual(['help-center'])
      expect(result!.apiUrl).toBe('http://example.com')
    })

    it('normalizes an array channelId: trims, drops empties, dedupes (order-preserving)', () => {
      const result = validateWidgetConfig({
        apiUrl: 'http://example.com',
        channelId: ['  help-center  ', '', 'docs', 'help-center', '  '],
      })
      expect(result).not.toBeNull()
      expect(result!.channelId).toEqual(['help-center', 'docs'])
    })
  })
})
