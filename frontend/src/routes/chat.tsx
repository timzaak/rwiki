import { createFileRoute } from '@tanstack/react-router'

import { ChatPanel } from '@/components/chat/chat-panel'

export const Route = createFileRoute('/chat')({
  component: ChatPage,
})

function ChatPage() {
  return (
    <div className="flex h-screen flex-col">
      <div className="flex items-center justify-between border-b px-4 py-2">
        <h1 className="text-sm font-semibold">Rwiki Chat</h1>
      </div>

      <div className="min-h-0 flex-1">
        <ChatPanel showHeader={false} />
      </div>
    </div>
  )
}
