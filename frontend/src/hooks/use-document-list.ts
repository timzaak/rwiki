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

export function useDocumentList() {
  const [documents, setDocuments] = useState<DocumentListItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refreshList = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await listDocuments()
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
  }, [])

  useEffect(() => {
    void refreshList()
  }, [refreshList])

  return { documents, loading, error, refreshList }
}
