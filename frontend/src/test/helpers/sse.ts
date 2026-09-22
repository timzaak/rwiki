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

/**
 * An SSE Response the test controls: streams the given events, then stays
 * open until the test enqueues more events or closes it — the shape for
 * interrupt scenarios, where the user acts while the answer is still
 * streaming.
 */
export function createControllableSseResponse(
  events: Array<{ event: string; data: unknown }>,
): {
  response: Response
  enqueueEvent: (event: string, data: unknown) => void
  close: () => void
} {
  const encoder = new TextEncoder()
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined
  const stream = new ReadableStream({
    start(c) {
      controller = c
      for (const evt of events) {
        c.enqueue(
          encoder.encode(
            `event: ${evt.event}\ndata: ${JSON.stringify(evt.data)}\n\n`,
          ),
        )
      }
      // Do NOT close — the stream stays open until the test closes it
    },
  })
  const response = new Response(stream, {
    headers: {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
    },
  })
  return {
    response,
    enqueueEvent: (event, data) =>
      controller!.enqueue(
        encoder.encode(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`),
      ),
    close: () => controller!.close(),
  }
}

/**
 * Streams the given events and then stays open until aborted.
 */
export function createHangingSseResponse(
  events: Array<{ event: string; data: unknown }>,
): Response {
  return createControllableSseResponse(events).response
}
