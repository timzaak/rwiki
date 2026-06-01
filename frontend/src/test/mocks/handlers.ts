import { http, HttpResponse } from 'msw'

export const handlers = [
  // Default chat SSE handler — returns a simple SSE stream
  http.post('/api/chat', async ({ request }) => {
    const body = (await request.json()) as { message?: string }

    if (!body?.message?.trim()) {
      return HttpResponse.json(
        { code: 400, message: '消息不能为空' },
        { status: 400 }
      )
    }

    const stream = new ReadableStream({
      start(controller) {
        const encoder = new TextEncoder()

        // session event
        controller.enqueue(
          encoder.encode(
            'event: session\ndata: {"sessionId":"test-session-1"}\n\n'
          )
        )
        // chunk event
        controller.enqueue(
          encoder.encode('event: chunk\ndata: {"content":"Hello"}\n\n')
        )
        // done event
        controller.enqueue(encoder.encode('event: done\ndata: {}\n\n'))

        controller.close()
      },
    })

    return new Response(stream, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
      },
    })
  }),
]
