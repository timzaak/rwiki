/**
 * 文档列表获取 hook（batch-refresh admin 页专用）
 *
 * Rule 7 偏差：仓库未安装 @tanstack/react-query，数据访问沿用既有约定
 * （自定义 hook + 生成客户端 + 局部 state，见 dev.md 偏差说明）。
 *
 * 401 由 FE-D01 的 client.interceptors.error 全局清理 Key 并跳 /auth/login，
 * 本 hook 不处理鉴权重定向，只在 fetch reject 时设置 error。
 */
import { useCallback, useEffect, useState } from 'react'
import { listDocuments } from '@/lib/api-generated/sdk.gen'
import type { DocumentListItem } from '@/lib/api-generated/types.gen'

/**
 * @param siteId 当前管理后台站点。为 null 时跳过请求（列表保持空、loading=false），
 *   避免向必填 siteId 的端点发送非法载荷。非 null 时按该 siteId 拉取，切换站点自动重取。
 */
export function useDocumentList(siteId: string | null) {
  const [documents, setDocuments] = useState<DocumentListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refreshList = useCallback(async () => {
    // siteId 为 null（尚未选定/无可用站点）时不发请求。
    if (siteId === null) {
      setDocuments([])
      setLoading(false)
      setError(null)
      return
    }
    setLoading(true)
    setError(null)
    try {
      const result = await listDocuments({ query: { siteId } })
      if (result.error) {
        setError('Failed to load')
      } else if (result.data) {
        setDocuments(result.data.documents)
      }
    } catch {
      setError('Failed to load')
    } finally {
      setLoading(false)
    }
  }, [siteId])

  useEffect(() => {
    void refreshList()
  }, [refreshList])

  return { documents, loading, error, refreshList }
}
