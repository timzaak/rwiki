/**
 * Creates a mock SSE ReadableStream that emits the given events.
 * Each event is formatted as "event: {type}\ndata: {json}\n\n".
 */
export function createSseStream(
  events: Array<{ event: string; data: unknown }>,
): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      const encoder = new TextEncoder()
      for (const evt of events) {
        controller.enqueue(
          encoder.encode(
            `event: ${evt.event}\ndata: ${JSON.stringify(evt.data)}\n\n`,
          ),
        )
      }
      controller.close()
    },
  })
}

/**
 * Creates a mock SSE Response for MSW handlers.
 */
export function createSseResponse(
  events: Array<{ event: string; data: unknown }>,
): Response {
  return new Response(createSseStream(events), {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    },
  })
}
