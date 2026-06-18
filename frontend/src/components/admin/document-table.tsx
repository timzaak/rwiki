import type { DocumentListItem } from '@/lib/api-generated/types.gen'
import { LoaderCircleIcon } from 'lucide-react'
import { formatDate } from '@/lib/format'

export type DocumentStatusFilter =
  | 'all'
  | 'draft'
  | 'published'
  | 'processing'
  | 'failed'

export interface DocumentTableProps {
  documents: DocumentListItem[]
  loading: boolean
  error: string | null
  selectedIds: Set<string>
  onSelectionChange: (next: Set<string>) => void
  statusFilter: DocumentStatusFilter
  onFilterChange: (next: DocumentStatusFilter) => void
}

const STATUS_FILTER_OPTIONS: ReadonlyArray<{
  value: DocumentStatusFilter
  label: string
}> = [
  { value: 'all', label: 'All' },
  { value: 'draft', label: 'Draft' },
  { value: 'published', label: 'Published' },
  { value: 'processing', label: 'Processing' },
  { value: 'failed', label: 'Failed' },
]

export function DocumentTable({
  documents,
  loading,
  error,
  selectedIds,
  onSelectionChange,
  statusFilter,
  onFilterChange,
}: DocumentTableProps) {
  function toggleOne(id: string) {
    const next = new Set(selectedIds)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    onSelectionChange(next)
  }

  function toggleAll() {
    const visibleIds = documents.map((doc) => doc.id)
    const allSelected =
      visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id))
    const next = new Set(selectedIds)
    if (allSelected) {
      for (const id of visibleIds) next.delete(id)
    } else {
      for (const id of visibleIds) next.add(id)
    }
    onSelectionChange(next)
  }

  if (loading) {
    return (
      <div
        data-testid="loading-spinner"
        className="flex items-center justify-center gap-2 py-12 text-muted-foreground"
      >
        <LoaderCircleIcon className="size-4 animate-spin" />
        <span className="text-sm">Loading documents...</span>
      </div>
    )
  }

  if (error) {
    return (
      <div
        data-testid="error-message"
        className="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive"
      >
        {error}
      </div>
    )
  }

  const visibleIds = documents.map((doc) => doc.id)
  const allSelected =
    visibleIds.length > 0 && visibleIds.every((id) => selectedIds.has(id))
  const partial =
    !allSelected && visibleIds.some((id) => selectedIds.has(id))

  return (
    <div data-testid="document-table" className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <label
          htmlFor="status-filter-select"
          className="text-xs font-medium text-muted-foreground"
        >
          Status
        </label>
        <select
          id="status-filter-select"
          data-testid="status-filter-select"
          value={statusFilter}
          onChange={(e) =>
            onFilterChange(e.target.value as DocumentStatusFilter)
          }
          className="h-8 rounded-lg border border-border/60 bg-card px-2 text-sm outline-none focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        >
          {STATUS_FILTER_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>

      {documents.length === 0 ? (
        <div
          data-testid="empty-state"
          className="flex flex-col items-center justify-center rounded-lg border border-dashed border-border/60 py-12 text-muted-foreground"
        >
          <p className="text-sm">No documents yet.</p>
        </div>
      ) : (
        <div className="overflow-x-auto rounded-lg border border-border/60">
          <table className="w-full border-collapse text-sm">
            <thead className="bg-muted/40 text-left text-xs text-muted-foreground">
              <tr>
                <th className="w-10 px-3 py-2">
                  <input
                    type="checkbox"
                    data-testid="select-all-checkbox"
                    checked={allSelected}
                    ref={(el) => {
                      if (el) el.indeterminate = partial
                    }}
                    onChange={toggleAll}
                    aria-label="Select all documents"
                  />
                </th>
                <th className="px-3 py-2 font-medium">File name</th>
                <th className="px-3 py-2 font-medium">Status</th>
                <th className="px-3 py-2 text-right font-medium">Rows</th>
                <th className="px-3 py-2 font-medium">Uploaded</th>
                <th className="px-3 py-2 font-medium">Error</th>
              </tr>
            </thead>
            <tbody>
              {documents.map((doc) => {
                const selected = selectedIds.has(doc.id)
                return (
                  <tr
                    key={doc.id}
                    data-testid="document-row"
                    className="border-t border-border/40"
                  >
                    <td className="px-3 py-2 align-middle">
                      <input
                        type="checkbox"
                        data-testid="document-select-checkbox"
                        value={doc.id}
                        checked={selected}
                        onChange={() => toggleOne(doc.id)}
                        aria-label={`Select ${doc.fileName}`}
                      />
                    </td>
                    <td className="px-3 py-2 align-middle font-medium">
                      {doc.fileName}
                    </td>
                    <td className="px-3 py-2 align-middle">{doc.status}</td>
                    <td className="px-3 py-2 text-right align-middle tabular-nums">
                      {doc.rowCount}
                    </td>
                    <td className="px-3 py-2 align-middle text-muted-foreground">
                      {formatDate(doc.createdAt)}
                    </td>
                    <td className="px-3 py-2 align-middle text-muted-foreground">
                      {doc.errorMessage ?? '—'}
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}
