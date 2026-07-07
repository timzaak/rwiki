import { useEffect, useState } from 'react'
import { Link, createFileRoute } from '@tanstack/react-router'
import { ArrowRightIcon, LoaderCircleIcon } from 'lucide-react'

import { listChannels } from '@/lib/api-generated'
import type { ChannelItem } from '@/lib/api-generated/types.gen'

export const Route = createFileRoute('/')({
  component: HomeRoute,
})

type LoadState =
  | { status: 'loading' }
  | { status: 'ready'; channels: ChannelItem[] }
  | { status: 'error' }
  | { status: 'empty' }

function HomeRoute() {
  const [state, setState] = useState<LoadState>({ status: 'loading' })

  const loadChannels = () => {
    setState({ status: 'loading' })
    let cancelled = false
    listChannels()
      .then((result) => {
        if (cancelled) return
        const channels = result.data?.channels ?? []
        setState(
          channels.length === 0 ? { status: 'empty' } : { status: 'ready', channels },
        )
      })
      .catch(() => {
        if (!cancelled) setState({ status: 'error' })
      })
    return () => {
      cancelled = true
    }
  }

  useEffect(() => loadChannels(), [])

  return (
    <div className="relative flex min-h-screen flex-col overflow-hidden">
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        <div className="absolute -top-40 -right-40 h-96 w-96 rounded-full bg-primary/8 blur-3xl" />
        <div className="absolute -bottom-32 -left-32 h-80 w-80 rounded-full bg-primary/5 blur-3xl" />
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_1px_1px,oklch(0.5_0.01_260_/_0.04)_1px,transparent_0)] bg-[length:32px_32px]" />
      </div>

      <nav className="relative z-10 flex items-center justify-between px-6 py-5 md:px-12 lg:px-20">
        <div className="flex items-center gap-2.5">
          <div className="flex size-8 items-center justify-center rounded-lg bg-primary">
            <span className="font-serif text-sm font-bold text-primary-foreground">R</span>
          </div>
          <span className="font-serif text-lg font-semibold tracking-tight">Rwiki</span>
        </div>
      </nav>

      <main className="relative z-10 flex flex-1 flex-col items-center justify-center px-6 pb-20 pt-8 md:px-12">
        <div className="w-full max-w-3xl text-center">
          <div className="animate-fade-in mb-6 inline-flex items-center gap-2 rounded-full border border-border/60 bg-card/50 px-4 py-1.5 text-xs font-medium tracking-wide text-muted-foreground backdrop-blur-sm">
            <span className="inline-block size-1.5 rounded-full bg-primary animate-glow-pulse" />
            RAG + Agent Knowledge Assistant
          </div>

          <h1 className="animate-slide-up font-serif text-5xl leading-tight font-bold tracking-tight md:text-7xl md:leading-[1.1]">
            Knowledge,
            <br />
            <span className="bg-gradient-to-r from-primary via-primary/80 to-primary/60 bg-clip-text text-transparent">
              Conversationally
            </span>
          </h1>

          <p className="animate-slide-up mx-auto mt-6 max-w-lg text-base leading-relaxed text-muted-foreground md:text-lg [animation-delay:100ms]">
            Choose a knowledge channel to start asking. Rwiki combines
            retrieval-augmented generation with intelligent agents to deliver
            precise, contextual answers.
          </p>

          <div className="animate-slide-up mt-10 [animation-delay:200ms]">
            <ChannelEntryList state={state} onRetry={loadChannels} />
          </div>
        </div>
      </main>

      <footer className="relative z-10 border-t border-border/40 py-6 text-center text-xs text-muted-foreground">
        Built with Rwiki &mdash; Knowledge, conversationally.
      </footer>
    </div>
  )
}

function ChannelEntryList({
  state,
  onRetry,
}: {
  state: LoadState
  onRetry: () => void
}) {
  if (state.status === 'loading') {
    return (
      <div
        data-testid="channel-list-loading"
        className="flex items-center justify-center gap-2 text-sm text-muted-foreground"
      >
        <LoaderCircleIcon className="size-4 animate-spin" />
        Loading channels…
      </div>
    )
  }

  if (state.status === 'error') {
    return (
      <div
        data-testid="channel-list-error"
        className="flex flex-col items-center gap-3 text-sm text-muted-foreground"
      >
        <span>无法加载频道列表</span>
        <button
          onClick={onRetry}
          className="rounded-full border border-border bg-card/80 px-5 py-2 text-sm font-medium backdrop-blur-sm transition-all hover:border-primary/30 hover:bg-card hover:shadow-sm"
        >
          重试
        </button>
      </div>
    )
  }

  if (state.status === 'empty') {
    return (
      <p
        data-testid="channel-list-empty"
        className="text-sm text-muted-foreground"
      >
        暂无可用频道，请联系管理员。
      </p>
    )
  }

  return (
    <ul className="flex flex-col items-center gap-3 sm:flex-row sm:flex-wrap sm:justify-center">
      {state.channels.map((channel) => (
        <li key={channel.id}>
          <Link
            to="/c/$channelId"
            params={{ channelId: channel.id }}
            data-testid={`channel-entry-${channel.id}`}
            className="group inline-flex items-center gap-2 rounded-full border border-border bg-card/80 px-6 py-3 text-sm font-medium backdrop-blur-sm transition-all hover:border-primary/30 hover:bg-card hover:shadow-md hover:scale-[1.02] active:scale-[0.98]"
          >
            <span data-testid="channel-entry" className="relative z-10">
              {channel.name}
            </span>
            <ArrowRightIcon className="relative z-10 size-4 transition-transform group-hover:translate-x-0.5" />
          </Link>
        </li>
      ))}
    </ul>
  )
}
