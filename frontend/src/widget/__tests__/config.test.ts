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
      welcomeMessage: 'Hi!',
    })
  })

  it('applies defaults when only apiUrl is provided', () => {
    const result = validateWidgetConfig({ apiUrl: 'http://example.com' })
    expect(result).toEqual({
      apiUrl: 'http://example.com',
      primaryColor: '#3b82f6',
      title: 'Chat Assistant',
      position: 'right',
    })
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
