import { useMemo, useState } from 'react'
import { LoaderCircleIcon, TrashIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import type {
  BatchStatusItem,
  DocumentListItem,
} from '@/lib/api-generated/types.gen'
import {
  batchUpdateStatus,
  deleteDocument,
} from '@/lib/api-generated/sdk.gen'

export interface BatchActionsProps {
  selectedIds: Set<string>
  documents: DocumentListItem[]
  onCompleted: () => void
}

const REASON_LABEL: Record<string, string> = {
  not_found: '文档不存在',
  invalid_status: '状态不允许',
}

function describeReason(reason: string | null | undefined): string | null {
  if (!reason) return null
  return REASON_LABEL[reason] ?? reason
}

export function BatchActions({
  selectedIds,
  documents,
  onCompleted,
}: BatchActionsProps) {
  const [submitting, setSubmitting] = useState(false)
  const [feedback, setFeedback] = useState<BatchStatusItem[] | null>(null)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  // 基于传入的 documents 构建 idToName：admin 传入完整列表，避免筛选/刷新后
  // 相关文档被排除导致 feedback 回退到裸 UUID。useMemo 避免每 render 重建。
  const idToName = useMemo(() => {
    const map = new Map<string, string>()
    for (const doc of documents) map.set(doc.id, doc.fileName)
    return map
  }, [documents])

  // 双保险：操作目标限定为「documents ∩ selectedIds」。即便调用方未在筛选
  // 切换时清空 selectedIds，也不会作用于不在 documents 中的隐藏文档。
  const selected = useMemo(
    () => documents.filter((doc) => selectedIds.has(doc.id)),
    [documents, selectedIds],
  )
  const disabled = selected.length === 0 || submitting

  async function submitBatch(action: 'publish' | 'unpublish') {
    // CORE REGRESSION: exactly one POST /api/documents/batch-status per batch.
    // Group selected ids by target action, submit publish + unpublish in a
    // single combined payload. Never call publishDocument/unpublishDocument.
    const publish =
      action === 'publish'
        ? selected.filter((d) => d.status === 'draft').map((d) => d.id)
        : []
    const unpublish =
      action === 'unpublish'
        ? selected.filter((d) => d.status === 'published').map((d) => d.id)
        : []
    if (publish.length === 0 && unpublish.length === 0) return

    setSubmitting(true)
    setFeedback(null)
    setError(null)
    try {
      const result = await batchUpdateStatus({ body: { publish, unpublish } })
      if (result.error || (result.response && !result.response.ok)) {
        setError('批量操作失败')
      } else if (result.data) {
        setFeedback(result.data.results)
        onCompleted()
      }
    } catch {
      setError('批量操作失败')
    } finally {
      setSubmitting(false)
    }
  }

  async function submitDelete() {
    setSubmitting(true)
    setDeleteError(null)
    try {
      // No batch-delete endpoint; delete per-id concurrently. 限定为当前可见
      // （即传入 documents）的选中项，避免删除被筛选排除的隐藏文档。
      await Promise.all(
        selected.map((doc) =>
          deleteDocument({ path: { documentId: doc.id } }),
        ),
      )
      onCompleted()
    } catch {
      setDeleteError('部分删除失败')
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div
      data-testid="batch-actions"
      className="flex flex-col gap-3 rounded-lg border border-border/60 bg-card p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="default"
          size="lg"
          data-testid="batch-publish-button"
          onClick={() => submitBatch('publish')}
          disabled={disabled}
        >
          {submitting ? (
            <LoaderCircleIcon className="size-4 animate-spin" />
          ) : null}
          批量上线
        </Button>
        <Button
          type="button"
          variant="outline"
          size="lg"
          data-testid="batch-unpublish-button"
          onClick={() => submitBatch('unpublish')}
          disabled={disabled}
        >
          批量下线
        </Button>
        <Button
          type="button"
          variant="destructive"
          size="lg"
          data-testid="batch-delete-button"
          onClick={submitDelete}
          disabled={disabled}
        >
          <TrashIcon className="size-4" />
          删除
        </Button>
        <span className="text-xs text-muted-foreground">
          已选 {selected.length} 项
        </span>
      </div>

      {error ? (
        <div
          data-testid="batch-error"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </div>
      ) : null}

      {deleteError ? (
        <div
          data-testid="batch-delete-error"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {deleteError}
        </div>
      ) : null}

      {feedback && feedback.length > 0 ? (
        <ul
          data-testid="batch-feedback"
          className="flex flex-col divide-y divide-border/40 rounded-lg border border-border/60 text-sm"
        >
          {feedback.map((item) => {
            const label = idToName.get(item.documentId) ?? item.documentId
            const reason = describeReason(item.reason)
            return (
              <li
                key={item.documentId}
                data-testid="batch-feedback-item"
                className="flex items-center justify-between gap-3 px-3 py-2"
              >
                <span className="truncate font-medium">{label}</span>
                <span className="flex items-center gap-2 text-xs">
                  <span
                    className={
                      item.applied
                        ? 'text-emerald-600 dark:text-emerald-400'
                        : 'text-muted-foreground'
                    }
                  >
                    {item.applied ? '已生效' : '未生效'}
                  </span>
                  {reason ? (
                    <span className="text-muted-foreground">{reason}</span>
                  ) : null}
                </span>
              </li>
            )
          })}
        </ul>
      ) : null}
    </div>
  )
}
