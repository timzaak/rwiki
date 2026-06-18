/**
 * Shared formatting helpers (table presentation, etc.).
 */

export function formatDate(iso: string): string {
  const date = new Date(iso)
  if (Number.isNaN(date.getTime())) return iso
  return date.toLocaleString()
}
