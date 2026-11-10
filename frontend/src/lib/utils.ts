import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

// crypto.randomUUID requires a secure context (https / localhost). Widget
// hosts on plain http or opaque origins (about:blank, sandboxed iframes)
// don't have it — fall back to a UUID-v4-shaped id from getRandomValues,
// which is available on every supported browser.
export function generateId(): string {
  const c = globalThis.crypto
  // randomUUID is missing in insecure contexts — the truthiness check stays
  // runtime-honest even though the DOM types always declare it.
  if (c?.randomUUID) {
    return c.randomUUID()
  }
  const bytes = new Uint8Array(16)
  c!.getRandomValues(bytes)
  bytes[6] = (bytes[6] & 0x3f) | 0x40
  bytes[8] = (bytes[8] & 0x3f) | 0x80
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('')
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`
}
