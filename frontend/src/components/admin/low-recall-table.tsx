import { Fragment, useState } from 'react'
import { LoaderCircleIcon } from 'lucide-react'
import type { LowRecallRecord } from '@/lib/api-generated/types.gen'
import { formatDate } from '@/lib/format'

/**
 * low-recall 表格（受控状态组件，结构对齐 DocumentTable）。
 *
 * 三态（loading / error / empty）由本组件互斥渲染并持有 testid，
 * 列表页不再重复这些 testid（参照 document-table.tsx 的状态分支约定）。
 */
export interface LowRecallTableProps {
  items: LowRecallRecord[]
  loading: boolean
  error: string | null
}

function formatTopScore(score: number | null | undefined): string {
  if (score === null || score === undefined) return 'N/A'
  return score.toFixed(4)
}

function truncate(text: string, max = 60): string {
  if (text.length <= max) return text
  return `${text.slice(0, max)}…`
}

export function LowRecallTable({ items, loading, error }: LowRecallTableProps) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set())

  function toggleRow(id: number) {
    setExpanded((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  if (loading) {
    return (
      <div
        data-testid="low-recall-loading"
        className="flex items-center justify-center gap-2 py-12 text-muted-foreground"
      >
        <LoaderCircleIcon className="size-4 animate-spin" />
        <span className="text-sm">Loading low-recall records...</span>
      </div>
    )
  }

  if (error) {
    return (
      <div
        data-testid="low-recall-error"
        className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive"
      >
        {error}
      </div>
    )
  }

  if (items.length === 0) {
    return (
      <div
        data-testid="low-recall-empty"
        className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border/60 py-12 text-muted-foreground"
      >
        <p className="text-sm">暂无低相关召回记录</p>
      </div>
    )
  }

  return (
    <div
      data-testid="low-recall-list"
      className="overflow-x-auto rounded-lg border border-border/60"
    >
      <table className="w-full border-collapse text-sm">
        <thead className="bg-muted/40 text-left text-xs text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">Query</th>
            <th className="px-3 py-2 text-right font-medium">Top score</th>
            <th className="px-3 py-2 text-right font-medium">Results</th>
            <th className="px-3 py-2 font-medium">Created</th>
            <th className="px-3 py-2 font-medium">Sources</th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => {
            const isOpen = expanded.has(item.id)
            const firstSource = item.sources[0]
            return (
              <Fragment key={item.id}>
                <tr
                  data-testid="low-recall-row"
                  className="border-t border-border/40"
                >
                  <td
                    className="px-3 py-2 align-middle font-medium"
                    title={item.query}
                  >
                    {truncate(item.query)}
                  </td>
                  <td className="px-3 py-2 text-right align-middle tabular-nums">
                    {formatTopScore(item.topScore)}
                  </td>
                  <td className="px-3 py-2 text-right align-middle tabular-nums">
                    {item.resultCount}
                  </td>
                  <td className="px-3 py-2 align-middle text-muted-foreground">
                    {formatDate(item.createdAt)}
                  </td>
                  <td className="px-3 py-2 align-middle">
                    {item.sources.length === 0 ? (
                      <span className="text-muted-foreground">—</span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => toggleRow(item.id)}
                        className="text-xs text-primary underline-offset-2 hover:underline"
                      >
                        {firstSource?.title ?? '—'}
                        {item.sources.length > 1
                          ? ` (+${item.sources.length - 1})`
                          : ''}
                      </button>
                    )}
                  </td>
                </tr>
                {isOpen && item.sources.length > 0 ? (
                  <tr
                    key={`${item.id}-sources`}
                    data-testid="low-recall-sources"
                    className="border-t border-border/40 bg-muted/20"
                  >
                    <td colSpan={5} className="px-3 py-2">
                      <ul className="flex flex-col gap-1 text-xs">
                        {item.sources.map((source) => (
                          <li
                            key={`${source.documentId}:${source.chunkId}`}
                            className="flex items-center gap-3"
                          >
                            <span className="font-mono text-muted-foreground">
                              {source.documentId}
                            </span>
                            <span className="flex-1 truncate" title={source.title}>
                              {source.title}
                            </span>
                            <span className="tabular-nums text-muted-foreground">
                              {source.score.toFixed(4)}
                            </span>
                          </li>
                        ))}
                      </ul>
                    </td>
                  </tr>
                ) : null}
              </Fragment>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
