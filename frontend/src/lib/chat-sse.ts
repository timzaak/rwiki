/**
 * Classifies a parsed chat SSE payload into the event type the UI reacts to.
 * Shared by the admin hook and the widget hook: the classification order is
 * part of the wire protocol and must stay identical on both sides.
 */
export function detectEventType(
  data: unknown,
): 'session' | 'chunk' | 'suggestions' | 'contentEnd' | 'error' | 'done' {
  if (typeof data !== 'object' || data === null) return 'done'
  const record = data as Record<string, unknown>
  if ('sessionId' in record && record.sessionId) return 'session'
  if ('content' in record && record.content !== undefined) return 'chunk'
  if ('suggestions' in record && Array.isArray(record.suggestions))
    return 'suggestions'
  if ('contentEnd' in record && record.contentEnd === true) return 'contentEnd'
  if ('message' in record && record.message) return 'error'
  return 'done'
}
