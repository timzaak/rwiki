import { useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useLowRecallRecords } from '@/hooks/use-low-recall-records'
import { LowRecallFilters } from '@/components/admin/low-recall-filters'
import { LowRecallTable } from '@/components/admin/low-recall-table'
import { Button } from '@/components/ui/button'
import { useAdminSite } from '@/lib/admin-site-context'

// 无自身 beforeLoad：守卫集中在父 route `routes/admin/route.tsx`，本子 route 继承。
export const Route = createFileRoute('/admin/low-recall')({
  component: LowRecallPage,
})

const DEFAULT_LIMIT = 20

function LowRecallPage() {
  // 全局站点上下文：siteId 为 null 时 useLowRecallRecords 跳过请求。
  const { siteId } = useAdminSite()
  const [minScore, setMinScore] = useState<number | null>(null)
  const [maxScore, setMaxScore] = useState<number | null>(null)
  const [from, setFrom] = useState<string | null>(null)
  const [to, setTo] = useState<string | null>(null)
  const limit = DEFAULT_LIMIT
  const [offset, setOffset] = useState<number>(0)

  const { items, total, loading, error } = useLowRecallRecords({
    siteId,
    minScore,
    maxScore,
    from,
    to,
    limit,
    offset,
  })

  // Apply：先把 offset 重置为 0 再更新筛选态，避免越界（page 0 起）。
  // Filters 组件在 onApply 前已通过 onMinScore/onMaxScore/onFrom/onTo 上报最新值。
  const handleApply = () => {
    setOffset(0)
  }

  const canPrev = offset > 0 && !loading && error === null
  const canNext = offset + limit < total && !loading && error === null

  return (
    <div
      data-testid="low-recall-page"
      className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-8"
    >
      <header className="flex flex-col gap-1">
        <h1 className="font-serif text-2xl font-semibold tracking-tight">
          Low-Recall Records
        </h1>
        <p className="text-sm text-muted-foreground">
          Queries whose retrieved chunks scored below threshold. Filter by score
          range and time window to inspect recall quality.
        </p>
      </header>

      <LowRecallFilters
        minScore={minScore}
        maxScore={maxScore}
        from={from}
        to={to}
        onMinScore={setMinScore}
        onMaxScore={setMaxScore}
        onFrom={setFrom}
        onTo={setTo}
        onApply={handleApply}
      />

      <LowRecallTable items={items} loading={loading} error={error} />

      <div className="flex items-center justify-between text-sm text-muted-foreground">
        <span data-testid="low-recall-total">Total: {total}</span>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="outline"
            data-testid="low-recall-prev"
            onClick={() => setOffset((prev) => Math.max(0, prev - limit))}
            disabled={!canPrev}
          >
            Prev
          </Button>
          <Button
            type="button"
            variant="outline"
            data-testid="low-recall-next"
            onClick={() => setOffset((prev) => prev + limit)}
            disabled={!canNext}
          >
            Next
          </Button>
        </div>
      </div>
    </div>
  )
}
