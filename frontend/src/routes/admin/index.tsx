import { createFileRoute } from '@tanstack/react-router'
import { useMemo, useState } from 'react'
import { useDocumentList } from '@/hooks/use-document-list'
import {
  DocumentTable,
  type DocumentStatusFilter,
} from '@/components/admin/document-table'
import { UploadDocument } from '@/components/admin/upload-document'
import { BatchActions } from '@/components/admin/batch-actions'
import { useAdminSite } from '@/lib/admin-site-context'

export const Route = createFileRoute('/admin/')({
  // 守卫集中在父 route `routes/admin/route.tsx` 的 `beforeLoad`，
  // 本子 route 继承，不重复、不靠组件内 useEffect 兜底。
  component: AdminComponent,
})

function AdminComponent() {
  // 全局站点上下文：siteId 为 null 时 useDocumentList 跳过请求（列表保持空）。
  const { siteId } = useAdminSite()
  const { documents, loading, error, refreshList } = useDocumentList(siteId)
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [statusFilter, setStatusFilter] = useState<DocumentStatusFilter>('all')

  const visible = useMemo(
    () =>
      statusFilter === 'all'
        ? documents
        : documents.filter((doc) => doc.status === statusFilter),
    [documents, statusFilter],
  )

  // 切换 status filter 必须同时清空选择集：否则被筛选排除的已选文档会变成
  // 不可见但仍在 selectedIds 中，BatchActions 的删除会作用到不可见文档（数据丢失）。
  function handleFilterChange(next: DocumentStatusFilter) {
    setSelectedIds(new Set())
    setStatusFilter(next)
  }

  // 列表级 fetch 状态（loading / error）由本页持有并打 document-list-* testid，
  // 供 test slot（FE-T05 step 3.6 断言 document-list-error）。fetch 成功后（含
  // 筛选后为空）交由 DocumentTable 渲染：它自带 status-filter select（必须常驻
  // 可见，否则筛选到空集时无法切回 all）与 empty-state。因此本页不另渲
  // document-list-empty——table 的 empty-state 已覆盖，且 FE-T05 未断言该 testid。
  function handleBatchCompleted() {
    setSelectedIds(new Set())
    void refreshList()
  }

  return (
    <div
      data-testid="admin-page"
      className="mx-auto flex w-full max-w-5xl flex-col gap-6 px-6 py-8"
    >
      <header className="flex items-start justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="font-serif text-2xl font-semibold tracking-tight">
            Document Management
          </h1>
          <p className="text-sm text-muted-foreground">
            Upload and delete documents, and batch-control publish status.
          </p>
        </div>
        <UploadDocument onUploaded={refreshList} />
      </header>

      <BatchActions
        selectedIds={selectedIds}
        documents={documents}
        onCompleted={handleBatchCompleted}
      />

      {loading ? (
        <div
          data-testid="document-list-loading"
          className="rounded-lg border border-border/60 bg-card px-4 py-12 text-center text-sm text-muted-foreground"
        >
          Loading…
        </div>
      ) : error ? (
        <div
          data-testid="document-list-error"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive"
        >
          {error}
        </div>
      ) : (
        <DocumentTable
          documents={visible}
          loading={false}
          error={null}
          selectedIds={selectedIds}
          onSelectionChange={setSelectedIds}
          statusFilter={statusFilter}
          onFilterChange={handleFilterChange}
        />
      )}
    </div>
  )
}
