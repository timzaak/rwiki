import { createFileRoute, Link } from '@tanstack/react-router'

export const Route = createFileRoute('/')({
  component: HomeRoute,
})

function HomeRoute() {
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
        <Link
          to="/chat"
          className="group flex items-center gap-2 rounded-full border border-border bg-card/80 px-5 py-2 text-sm font-medium backdrop-blur-sm transition-all hover:border-primary/30 hover:bg-card hover:shadow-sm"
        >
          <span>Open Chat</span>
          <svg
            className="size-4 transition-transform group-hover:translate-x-0.5"
            fill="none"
            viewBox="0 0 24 24"
            strokeWidth={1.5}
            stroke="currentColor"
          >
            <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
          </svg>
        </Link>
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
            Ask your knowledge base anything. Rwiki combines retrieval-augmented generation
            with intelligent agents to deliver precise, contextual answers.
          </p>

          <div className="animate-slide-up mt-10 flex flex-col items-center gap-4 sm:flex-row sm:justify-center [animation-delay:200ms]">
            <Link
              to="/chat"
              className="group relative inline-flex items-center gap-2 overflow-hidden rounded-full bg-primary px-7 py-3 text-sm font-semibold text-primary-foreground shadow-lg transition-all hover:shadow-xl hover:scale-[1.02] active:scale-[0.98]"
            >
              <span className="relative z-10">Start a Conversation</span>
              <svg
                className="relative z-10 size-4 transition-transform group-hover:translate-x-0.5"
                fill="none"
                viewBox="0 0 24 24"
                strokeWidth={2}
                stroke="currentColor"
              >
                <path strokeLinecap="round" strokeLinejoin="round" d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
              </svg>
              <div className="absolute inset-0 bg-gradient-to-r from-primary/0 via-white/10 to-primary/0 translate-x-[-200%] group-hover:translate-x-[200%] transition-transform duration-700" />
            </Link>
          </div>

          <div className="animate-slide-up mt-16 grid grid-cols-1 gap-6 sm:grid-cols-3 [animation-delay:300ms]">
            <FeatureCard
              icon={
                <svg className="size-5" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.042A8.967 8.967 0 0 0 6 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 0 1 6 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 0 1 6-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0 0 18 18a8.967 8.967 0 0 0-6 2.292m0-14.25v14.25" />
                </svg>
              }
              title="RAG-Powered"
              description="Retrieval-augmented generation grounds every answer in your documents."
            />
            <FeatureCard
              icon={
                <svg className="size-5" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M9.813 15.904 9 18.75l-.813-2.846a4.5 4.5 0 0 0-3.09-3.09L2.25 12l2.846-.813a4.5 4.5 0 0 0 3.09-3.09L9 5.25l.813 2.846a4.5 4.5 0 0 0 3.09 3.09L15.75 12l-2.846.813a4.5 4.5 0 0 0-3.09 3.09ZM18.259 8.715 18 9.75l-.259-1.035a3.375 3.375 0 0 0-2.455-2.456L14.25 6l1.036-.259a3.375 3.375 0 0 0 2.455-2.456L18 2.25l.259 1.035a3.375 3.375 0 0 0 2.456 2.456L21.75 6l-1.035.259a3.375 3.375 0 0 0-2.456 2.456ZM16.894 20.567 16.5 21.75l-.394-1.183a2.25 2.25 0 0 0-1.423-1.423L13.5 18.75l1.183-.394a2.25 2.25 0 0 0 1.423-1.423l.394-1.183.394 1.183a2.25 2.25 0 0 0 1.423 1.423l1.183.394-1.183.394a2.25 2.25 0 0 0-1.423 1.423Z" />
                </svg>
              }
              title="Agent-Based"
              description="Intelligent agents decompose complex queries and orchestrate tools."
            />
            <FeatureCard
              icon={
                <svg className="size-5" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M17.25 6.75 22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3-4.5 16.5" />
                </svg>
              }
              title="API Integration"
              description="Natural language commands trigger real service APIs seamlessly."
            />
          </div>
        </div>
      </main>

      <footer className="relative z-10 border-t border-border/40 py-6 text-center text-xs text-muted-foreground">
        Built with Rwiki &mdash; Knowledge, conversationally.
      </footer>
    </div>
  )
}

function FeatureCard({ icon, title, description }: { icon: React.ReactNode; title: string; description: string }) {
  return (
    <div className="group rounded-xl border border-border/50 bg-card/50 p-5 text-left backdrop-blur-sm transition-all hover:border-primary/20 hover:bg-card/80 hover:shadow-md">
      <div className="mb-3 flex size-9 items-center justify-center rounded-lg bg-primary/10 text-primary transition-colors group-hover:bg-primary/15">
        {icon}
      </div>
      <h3 className="font-serif text-sm font-semibold tracking-tight">{title}</h3>
      <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">{description}</p>
    </div>
  )
}
