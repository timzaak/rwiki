import { describe, it, expect, vi, beforeEach } from 'vitest'

import { validateWidgetConfig } from '../config'

describe('validateWidgetConfig', () => {
  beforeEach(() => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
  })

  it('returns null and logs error when apiUrl is missing', () => {
    const result = validateWidgetConfig({})
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith('[RWikiChat] apiUrl is required')
  })

  it('returns null when apiUrl does not start with http:// or https://', () => {
    const result = validateWidgetConfig({ apiUrl: 'ftp://example.com' })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] apiUrl must start with http:// or https://',
    )
  })

  it('strips trailing slashes from apiUrl', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com/' })
    expect(result).not.toBeNull()
    expect(result!.apiUrl).toBe('http://example.com')
  })

  it('returns null when primaryColor is not a valid hex color', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
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
      primaryColor: '#ff5500',
    })
    expect(result).not.toBeNull()
    expect(result!.primaryColor).toBe('#ff5500')
  })

  it('returns null when position is invalid', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
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
      primaryColor: '#ff5500',
      title: 'My Bot',
      position: 'left',
      welcomeMessage: 'Hi!',
    })
    expect(result).toEqual({
      apiUrl: 'http://example.com',
      primaryColor: '#ff5500',
      title: 'My Bot',
      position: 'left',
      locale: 'en',
      welcomeMessage: 'Hi!',
    })
  })

  it('applies defaults when only apiUrl is provided', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com' })
    expect(result).toEqual({
      apiUrl: 'http://example.com',
      primaryColor: '#3b82f6',
      position: 'right',
      locale: 'en',
    })
  })

  it('does not include a title key when no title is provided (resolved at render time)', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com' })
    expect(result).not.toHaveProperty('title')
  })

  it('resolves locale from navigator.language by default (en-US -> en)', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com' })
    expect(result!.locale).toBe('en')
  })

  it('preserves an explicit supported locale', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      locale: 'zh-CN',
    })
    expect(result!.locale).toBe('zh-CN')
  })

  it('resolves an explicit locale prefix to a supported locale (zh-CN-Hans -> zh-CN)', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      locale: 'zh-CN-Hans',
    })
    expect(result!.locale).toBe('zh-CN')
  })

  it('resolves Chinese variants (zh-Hans / zh-TW / zh-HK / zh) to zh-CN', () => {
    for (const locale of ['zh-Hans', 'zh-TW', 'zh-HK', 'zh']) {
      const result = validateWidgetConfig({
        apiUrl: 'http://example.com',
        locale,
      })
      expect(result!.locale).toBe('zh-CN')
    }
  })

  it('falls back to en for an unsupported locale', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      locale: 'fr',
    })
    expect(result!.locale).toBe('en')
  })

  it('passes messages overrides through to the validated config', () => {
    const overrides = { inputPlaceholder: 'Custom' }
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      messages: overrides,
    })
    expect(result).toHaveProperty('messages', overrides)
  })

  it('returns null when locale is an empty string', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      locale: '',
    })
    expect(result).toBeNull()
    expect(console.error).toHaveBeenCalledWith(
      '[RWikiChat] locale must be a non-empty string',
    )
  })

  it('omits welcomeMessage when undefined', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com' })
    expect(result).not.toHaveProperty('welcomeMessage')
  })

  it('includes welcomeMessage when provided', () => {
    const result = validateWidgetConfig({
      apiUrl: 'http://example.com',
      welcomeMessage: 'How can I help?',
    })
    expect(result).toHaveProperty('welcomeMessage', 'How can I help?')
  })
})
