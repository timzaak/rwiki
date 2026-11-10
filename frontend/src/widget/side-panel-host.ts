// Host-page layout shift for the side panel display mode. The widget renders
// inside Shadow DOM, but pushing the host's document flow left needs root-level
// rules in the host document (scoped exception to the style-isolation rule):
// everything lives in one <style> with a fixed id plus one functional class on
// <html>, so open/close/destroy stay idempotent and leave no residue.
//
// The push threshold (40rem) must equal the panel's fullscreen fallback
// breakpoint (`max-sm:` in chat-side-panel.tsx): above it the panel docks and
// the host pushes; below it the panel is fullscreen and nothing pushes. A
// mismatch creates a viewport band where the docked panel covers content
// instead of pushing it.

export const SIDE_PANEL_STYLE_ID = 'rwiki-side-panel-style';
export const SIDE_PANEL_OPEN_CLASS = 'rwiki-side-panel-open';

const SIDE_PANEL_HOST_CSS = `:root { --rwiki-side-panel-width: min(420px, 75vw); }
@media (min-width: 40rem) {
  html { transition: margin-right 300ms ease-out; }
  html.rwiki-side-panel-open { margin-right: var(--rwiki-side-panel-width); }
}
@media (prefers-reduced-motion: reduce) {
  html { transition: none; }
}`;

// Matches the panel's exit animation; the style outlives the class removal so
// the margin transition can play out before the style element is dropped.
const CLOSE_SETTLE_MS = 300;

let removeStyleTimer: number | null = null;

// Held reference instead of an id lookup: the host page may legitimately
// contain an unrelated element with the same id, and a lookup would both
// skip our injection and later remove the host's element (PRD §4.1: the
// widget only ever removes what it injected itself).
let sidePanelStyle: HTMLStyleElement | null = null;

function installSidePanelStyle(): void {
  if (sidePanelStyle?.isConnected) return;
  const style = document.createElement('style');
  style.id = SIDE_PANEL_STYLE_ID;
  style.textContent = SIDE_PANEL_HOST_CSS;
  document.head.appendChild(style);
  sidePanelStyle = style;
}

// Toggle the host shift. Opening installs the style (if absent) and adds the
// open class in the same synchronous store notification, so the margin
// transition runs in lockstep with the panel slide-in. Closing drops the class
// immediately (the transition plays out) and removes the style element after
// the panel's exit animation; reopening cancels that pending removal.
export function setSidePanelOpen(open: boolean): void {
  if (open) {
    if (removeStyleTimer !== null) {
      clearTimeout(removeStyleTimer);
      removeStyleTimer = null;
    }
    installSidePanelStyle();
    document.documentElement.classList.add(SIDE_PANEL_OPEN_CLASS);
    return;
  }
  document.documentElement.classList.remove(SIDE_PANEL_OPEN_CLASS);
  if (removeStyleTimer !== null) clearTimeout(removeStyleTimer);
  removeStyleTimer = window.setTimeout(() => {
    removeStyleTimer = null;
    sidePanelStyle?.remove();
    sidePanelStyle = null;
  }, CLOSE_SETTLE_MS);
}

// Synchronous full cleanup for destroy(): no animation wait, no residue.
export function removeSidePanelHostInjection(): void {
  if (removeStyleTimer !== null) {
    clearTimeout(removeStyleTimer);
    removeStyleTimer = null;
  }
  document.documentElement.classList.remove(SIDE_PANEL_OPEN_CLASS);
  sidePanelStyle?.remove();
  sidePanelStyle = null;
}
