import { useEffect, useState, type FormEvent } from 'react'
import { Button } from '@/components/ui/button'

/**
 * low-recall 列表筛选区（受控）。
 *
 * 输入实时更新组件内本地 state（避免每次键入都打网络），
 * 仅在点击 Apply 时通过 onApply 通知父组件刷新查询。
 */
export interface LowRecallFiltersProps {
  minScore: number | null
  maxScore: number | null
  from: string | null
  to: string | null
  onMinScore: (next: number | null) => void
  onMaxScore: (next: number | null) => void
  onFrom: (next: string | null) => void
  onTo: (next: string | null) => void
  onApply: () => void
}

export function LowRecallFilters({
  minScore,
  maxScore,
  from,
  to,
  onMinScore,
  onMaxScore,
  onFrom,
  onTo,
  onApply,
}: LowRecallFiltersProps) {
  const [localMinScore, setLocalMinScore] = useState<string>(
    minScore === null ? '' : String(minScore),
  )
  const [localMaxScore, setLocalMaxScore] = useState<string>(
    maxScore === null ? '' : String(maxScore),
  )
  const [localFrom, setLocalFrom] = useState<string>(from ?? '')
  const [localTo, setLocalTo] = useState<string>(to ?? '')

  // Sync local state when controlled props change from the outside (e.g. the
  // parent resets filters). Inputs are typed into local state to avoid a fetch
  // per keystroke, but they must still track external prop updates.
  useEffect(() => {
    setLocalMinScore(minScore == null ? '' : String(minScore))
  }, [minScore])
  useEffect(() => {
    setLocalMaxScore(maxScore == null ? '' : String(maxScore))
  }, [maxScore])
  useEffect(() => {
    setLocalFrom(from ?? '')
  }, [from])
  useEffect(() => {
    setLocalTo(to ?? '')
  }, [to])

  function handleSubmit(e: FormEvent) {
    e.preventDefault()

    const parsedMin = localMinScore.trim() === '' ? null : Number(localMinScore)
    const parsedMax = localMaxScore.trim() === '' ? null : Number(localMaxScore)
    onMinScore(parsedMin === null || Number.isNaN(parsedMin) ? null : parsedMin)
    onMaxScore(parsedMax === null || Number.isNaN(parsedMax) ? null : parsedMax)
    onFrom(localFrom.trim() === '' ? null : localFrom.trim())
    onTo(localTo.trim() === '' ? null : localTo.trim())
    onApply()
  }

  return (
    <form
      onSubmit={handleSubmit}
      className="flex flex-wrap items-end gap-3 rounded-lg border border-border/60 bg-card px-4 py-3"
    >
      <label className="flex flex-col gap-1 text-xs text-muted-foreground">
        Min score
        <input
          type="number"
          step="0.01"
          data-testid="low-recall-filter-min-score"
          value={localMinScore}
          onChange={(e) => setLocalMinScore(e.target.value)}
          className="h-8 w-28 rounded-lg border border-border/60 bg-background px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-muted-foreground">
        Max score
        <input
          type="number"
          step="0.01"
          data-testid="low-recall-filter-max-score"
          value={localMaxScore}
          onChange={(e) => setLocalMaxScore(e.target.value)}
          className="h-8 w-28 rounded-lg border border-border/60 bg-background px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-muted-foreground">
        From
        <input
          type="datetime-local"
          data-testid="low-recall-filter-from"
          value={localFrom}
          onChange={(e) => setLocalFrom(e.target.value)}
          className="h-8 rounded-lg border border-border/60 bg-background px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-muted-foreground">
        To
        <input
          type="datetime-local"
          data-testid="low-recall-filter-to"
          value={localTo}
          onChange={(e) => setLocalTo(e.target.value)}
          className="h-8 rounded-lg border border-border/60 bg-background px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        />
      </label>
      <Button
        type="submit"
        variant="default"
        data-testid="low-recall-apply"
      >
        Apply
      </Button>
    </form>
  )
}
