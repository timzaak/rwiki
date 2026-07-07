import { useMemo, useState } from 'react'
import {
  CircleAlertIcon,
  CircleCheckIcon,
  LoaderCircleIcon,
  TrashIcon,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import type {
  BatchStatusItem,
  DocumentListItem,
} from '@/lib/api-generated/types.gen'
import {
  batchUpdateStatus,
  deleteDocument,
} from '@/lib/api-generated/sdk.gen'
import { useAdminSite } from '@/lib/admin-site-context'

export interface BatchActionsProps {
  selectedIds: Set<string>
  documents: DocumentListItem[]
  onCompleted: () => void
}

const REASON_LABEL: Record<string, string> = {
  not_found: 'Document not found',
  invalid_status: 'Invalid status',
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
  // 全局站点上下文：siteId 为 null（尚未选定/无可用站点）时禁用所有操作。
  // batch siteId 在 body（BatchStatusRequest），delete siteId 在 query。
  const { siteId } = useAdminSite()
  const [submitting, setSubmitting] = useState(false)
  const [feedback, setFeedback] = useState<BatchStatusItem[] | null>(null)
  const [lastAction, setLastAction] = useState<'publish' | 'unpublish' | null>(
    null,
  )
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
  const disabled = selected.length === 0 || submitting || siteId === null

  // Concise feedback: a single summary line, with only the failures enumerated
  // (success noise removed). Derivation is pure over `feedback`.
  const appliedCount = feedback?.filter((item) => item.applied).length ?? 0
  const failedItems = feedback?.filter((item) => !item.applied) ?? []
  const verb = lastAction === 'publish' ? 'Published' : 'Unpublished'
  const total = feedback?.length ?? 0
  const summary =
    failedItems.length === 0
      ? `${verb} ${appliedCount} ${appliedCount === 1 ? 'document' : 'documents'}.`
      : `${verb} ${appliedCount} of ${total} ${total === 1 ? 'document' : 'documents'}.`

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
    // siteId 在 body（BatchStatusRequest.siteId，必填）；为 null 时按钮已禁用。
    if (siteId === null) return

    setSubmitting(true)
    setLastAction(action)
    setFeedback(null)
    setError(null)
    try {
      const result = await batchUpdateStatus({
        body: { publish, unpublish, siteId },
      })
      if (result.error || (result.response && !result.response.ok)) {
        setError('Batch operation failed')
      } else if (result.data) {
        setFeedback(result.data.results)
        onCompleted()
      }
    } catch {
      setError('Batch operation failed')
    } finally {
      setSubmitting(false)
    }
  }

  async function submitDelete() {
    // siteId 在 query（必填）；为 null 时按钮已禁用。
    if (siteId === null) return
    setSubmitting(true)
    setDeleteError(null)
    try {
      // No batch-delete endpoint; delete per-id concurrently. 限定为当前可见
      // （即传入 documents）的选中项，避免删除被筛选排除的隐藏文档。
      await Promise.all(
        selected.map((doc) =>
          deleteDocument({
            path: { documentId: doc.id },
            query: { siteId },
          }),
        ),
      )
      onCompleted()
    } catch {
      setDeleteError('Some deletions failed')
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
          Batch Publish
        </Button>
        <Button
          type="button"
          variant="outline"
          size="lg"
          data-testid="batch-unpublish-button"
          onClick={() => submitBatch('unpublish')}
          disabled={disabled}
        >
          Batch Unpublish
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
          Delete
        </Button>
        <span className="text-xs text-muted-foreground">
          {selected.length} selected
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
        <div
          data-testid="batch-feedback"
          className="flex flex-col gap-2 rounded-lg border border-border/60 bg-card px-3 py-2.5 text-sm animate-fade-in"
        >
          <div className="flex items-center gap-2">
            {failedItems.length === 0 ? (
              <CircleCheckIcon className="size-4 text-emerald-600 dark:text-emerald-400" />
            ) : (
              <CircleAlertIcon className="size-4 text-amber-600 dark:text-amber-400" />
            )}
            <span className="font-medium">{summary}</span>
          </div>
          {failedItems.length > 0 ? (
            <ul className="flex flex-col gap-1 pl-6 text-xs text-muted-foreground">
              {failedItems.map((item) => {
                const label = idToName.get(item.documentId) ?? item.documentId
                const reason = describeReason(item.reason)
                return (
                  <li
                    key={item.documentId}
                    data-testid="batch-feedback-item"
                    className="flex flex-wrap items-center gap-1"
                  >
                    <span className="truncate text-foreground/80">{label}</span>
                    <span>— Not applied{reason ? ` · ${reason}` : ''}</span>
                  </li>
                )
              })}
            </ul>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
