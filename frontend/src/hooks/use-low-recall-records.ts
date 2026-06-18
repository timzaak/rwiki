/**
 * low-recall 查询 hook（admin Low-Recall Records 页专用）
 *
 * Rule 7 偏差：仓库未安装 @tanstack/react-query，数据访问沿用既有约定
 * （自定义 hook + 生成客户端 + 局部 state，见 dev.md 偏差说明）。
 *
 * 401 由 client.interceptors.error 全局清理 Key 并跳 /auth/login，
 * 本 hook 不处理鉴权重定向，只在 fetch reject 时设置 error。
 */
import { useEffect, useState } from 'react'
import { listLowRecallRecords } from '@/lib/api-generated/sdk.gen'
import type { LowRecallRecord } from '@/lib/api-generated/types.gen'

export interface UseLowRecallRecordsParams {
  minScore?: number | null
  maxScore?: number | null
  from?: string | null
  to?: string | null
  limit?: number
  offset?: number
}

export function useLowRecallRecords(params: UseLowRecallRecordsParams) {
  const { minScore, maxScore, from, to, limit, offset } = params
  const [items, setItems] = useState<LowRecallRecord[]>([])
  const [total, setTotal] = useState(0)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)
    // Reset stale data on every dep change so the Total label and table don't
    // show the previous page's values while the new fetch is in flight.
    setItems([])
    setTotal(0)
    listLowRecallRecords({
      query: {
        minScore: minScore ?? undefined,
        maxScore: maxScore ?? undefined,
        // Normalize to RFC3339 (UTC with offset) — backend parses with
        // chrono::DateTime::parse_from_rfc3339 which rejects naive strings.
        from: from ? new Date(from).toISOString() : undefined,
        to: to ? new Date(to).toISOString() : undefined,
        limit,
        offset,
      },
    })
      .then((result) => {
        if (cancelled) return
        if (result.error) {
          setError('Failed to load')
        } else if (result.data) {
          setItems(result.data.items)
          setTotal(result.data.total)
        }
      })
      .catch(() => {
        if (cancelled) return
        setError('Failed to load')
      })
      .finally(() => {
        if (cancelled) return
        setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [minScore, maxScore, from, to, limit, offset])

  return { items, total, loading, error }
}
