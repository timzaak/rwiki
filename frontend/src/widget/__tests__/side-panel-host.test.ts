import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'

import {
  SIDE_PANEL_OPEN_CLASS,
  SIDE_PANEL_STYLE_ID,
  removeSidePanelHostInjection,
  setSidePanelOpen,
} from '../side-panel-host'

function styleEl() {
  return document.getElementById(SIDE_PANEL_STYLE_ID)
}

function openClassOnRoot() {
  return document.documentElement.classList.contains(SIDE_PANEL_OPEN_CLASS)
}

function cleanHost() {
  document.getElementById(SIDE_PANEL_STYLE_ID)?.remove()
  document.documentElement.classList.remove(SIDE_PANEL_OPEN_CLASS)
}

beforeEach(() => {
  vi.useFakeTimers()
  cleanHost()
})

afterEach(() => {
  // Flush any pending removal timer so it cannot leak into the next test.
  vi.runAllTimers()
  vi.useRealTimers()
  cleanHost()
})

describe('side-panel-host open (US-INTG-018 scenario 2)', () => {
  it('installs one id-located style element and adds the root class', () => {
    setSidePanelOpen(true)

    const style = styleEl()
    expect(style).not.toBeNull()
    expect(style!.parentElement).toBe(document.head)
    // The panel width and the host margin must come from the same variable —
    // if this root rule disappears, panel and push drift apart.
    expect(style!.textContent).toContain('--rwiki-side-panel-width')
    // The push threshold must equal the panel's max-sm fullscreen breakpoint
    // (40rem = 640px); a mismatch creates a viewport band where the docked
    // panel covers host content instead of pushing it.
    expect(style!.textContent).toContain('@media (min-width: 40rem)')
    expect(openClassOnRoot()).toBe(true)
  })

  it('does not duplicate the style element on repeated opens', () => {
    setSidePanelOpen(true)
    setSidePanelOpen(true)

    expect(document.querySelectorAll(`#${SIDE_PANEL_STYLE_ID}`)).toHaveLength(1)
  })
})

describe('side-panel-host close (US-INTG-018 scenario 3: no residue)', () => {
  it('removes the class immediately but keeps the style until the exit transition ends', () => {
    setSidePanelOpen(true)

    setSidePanelOpen(false)

    // Class first (the margin transition plays out), style still present.
    expect(openClassOnRoot()).toBe(false)
    expect(styleEl()).not.toBeNull()

    vi.advanceTimersByTime(300)

    expect(styleEl()).toBeNull()
  })

  it('cancels the pending style removal when the panel reopens in the transition window', () => {
    setSidePanelOpen(true)
    setSidePanelOpen(false)
    setSidePanelOpen(true)

    vi.advanceTimersByTime(300)

    // Reopen cancels the removal: rapid toggling keeps exactly one style node
    // and the host stays shifted.
    expect(document.querySelectorAll(`#${SIDE_PANEL_STYLE_ID}`)).toHaveLength(1)
    expect(openClassOnRoot()).toBe(true)
  })

  it('leaves the host untouched after a full open/close cycle', () => {
    setSidePanelOpen(true)
    setSidePanelOpen(false)
    vi.advanceTimersByTime(300)

    expect(styleEl()).toBeNull()
    expect(openClassOnRoot()).toBe(false)
  })
})

describe('side-panel-host destroy cleanup (PRD §5.1-2 synchronous restore)', () => {
  it('clears class and style immediately while the panel is open — no animation wait', () => {
    setSidePanelOpen(true)

    removeSidePanelHostInjection()

    expect(openClassOnRoot()).toBe(false)
    expect(styleEl()).toBeNull()
    // No timer survives to fire later.
    vi.advanceTimersByTime(1000)
    expect(styleEl()).toBeNull()
  })

  it('cancels a pending close-removal timer so destroy is the final word', () => {
    setSidePanelOpen(true)
    setSidePanelOpen(false) // schedules removal at +300ms

    removeSidePanelHostInjection()

    expect(styleEl()).toBeNull()
    vi.advanceTimersByTime(1000)
    expect(styleEl()).toBeNull()
  })

  it('is a safe no-op when nothing was ever injected', () => {
    expect(() => removeSidePanelHostInjection()).not.toThrow()
    expect(styleEl()).toBeNull()
    expect(openClassOnRoot()).toBe(false)
  })
})
