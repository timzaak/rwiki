import { useRef, useState } from 'react'
import {
  CircleCheckIcon,
  CircleXIcon,
  FileIcon,
  LoaderCircleIcon,
  UploadCloudIcon,
  XIcon,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { uploadDocument } from '@/lib/api-generated/sdk.gen'

export interface UploadDocumentProps {
  /** Called after a successful upload so the parent can refresh the list. */
  onUploaded: () => void
}

type UploadStatus = 'pending' | 'uploading' | 'done' | 'error'

interface UploadItem {
  id: string
  file: File
  status: UploadStatus
}

function StatusIcon({ status }: { status: UploadStatus }) {
  switch (status) {
    case 'uploading':
      return (
        <LoaderCircleIcon className="size-4 shrink-0 animate-spin text-primary" />
      )
    case 'done':
      return (
        <CircleCheckIcon className="size-4 shrink-0 text-emerald-600 dark:text-emerald-400" />
      )
    case 'error':
      return <CircleXIcon className="size-4 shrink-0 text-destructive" />
    default:
      return <FileIcon className="size-4 shrink-0 text-muted-foreground" />
  }
}

export function UploadDocument({ onUploaded }: UploadDocumentProps) {
  const inputRef = useRef<HTMLInputElement>(null)
  const idRef = useRef(0)
  const [items, setItems] = useState<UploadItem[]>([])
  const [uploading, setUploading] = useState(false)

  const pending = items.filter((item) => item.status === 'pending')
  const done = items.filter((item) => item.status === 'done')
  const failed = items.filter((item) => item.status === 'error')
  const finished =
    !uploading && (done.length > 0 || failed.length > 0) && pending.length === 0

  // Stage one or more files. Terminal (done/error) items from a previous run
  // are dropped so each new selection starts a clean batch; still-pending or
  // in-flight items are kept.
  function stageFiles(fileList: FileList | null | undefined) {
    if (!fileList || fileList.length === 0) return
    const fresh: UploadItem[] = Array.from(fileList).map((file) => ({
      id: `u${(idRef.current += 1)}`,
      file,
      status: 'pending' as const,
    }))
    setItems((prev) => [
      ...prev.filter((i) => i.status === 'pending' || i.status === 'uploading'),
      ...fresh,
    ])
  }

  function updateStatus(id: string, status: UploadStatus) {
    setItems((prev) =>
      prev.map((item) => (item.id === id ? { ...item, status } : item)),
    )
  }

  function removeItem(id: string) {
    setItems((prev) => prev.filter((item) => item.id !== id))
  }

  function clearAll() {
    setItems([])
    if (inputRef.current) inputRef.current.value = ''
  }

  // Sequential upload: one POST /api/documents/upload per file, in order.
  // Sequential (not parallel) to avoid rate-limiting the embedding provider
  // and contending on SQLite writes. The table is refreshed once after the
  // whole batch, not per file.
  async function doUpload() {
    const queue = items.filter((item) => item.status === 'pending')
    if (queue.length === 0) return

    setUploading(true)
    let anyNew = false
    for (const item of queue) {
      updateStatus(item.id, 'uploading')
      try {
        // multipart serialization is handled by the generated SDK
        // (formDataBodySerializer + Content-Type cleared); no manual work here.
        const result = await uploadDocument({ body: { file: item.file } })
        if (result.error || (result.response && !result.response.ok)) {
          updateStatus(item.id, 'error')
        } else {
          updateStatus(item.id, 'done')
          anyNew = true
        }
      } catch {
        updateStatus(item.id, 'error')
      }
    }
    setUploading(false)
    if (anyNew) onUploaded()
  }

  return (
    <div data-testid="upload-document" className="relative">
      {/* Always-mounted hidden input so tests/drivers can target it directly. */}
      <input
        ref={inputRef}
        type="file"
        multiple
        data-testid="file-input"
        className="hidden"
        onChange={(e) => {
          stageFiles(e.target.files)
          // Reset so the same file can be re-selected in a later batch.
          e.target.value = ''
        }}
      />

      {/* Idle footprint = one button. Opens the picker and accepts drag-drop. */}
      <Button
        type="button"
        variant="default"
        size="lg"
        data-testid="upload-dropzone"
        disabled={uploading}
        onClick={() => inputRef.current?.click()}
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault()
          if (uploading) return
          stageFiles(e.dataTransfer.files)
        }}
      >
        <UploadCloudIcon />
        Upload document
      </Button>

      {/* Popover: appears only when files are staged. */}
      {items.length > 0 ? (
        <div className="absolute right-0 top-full z-20 mt-2 w-80 animate-fade-in rounded-lg border border-border/60 bg-card p-3 shadow-lg">
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs font-medium text-muted-foreground">
              {items.length} file{items.length === 1 ? '' : 's'}
            </span>
            <button
              type="button"
              aria-label="Clear all"
              className="rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground disabled:pointer-events-none disabled:opacity-40"
              disabled={uploading}
              onClick={clearAll}
            >
              <XIcon className="size-3.5" />
            </button>
          </div>

          <ul className="mt-2 flex max-h-56 flex-col gap-1 overflow-y-auto">
            {items.map((item) => (
              <li
                key={item.id}
                className="flex items-center gap-2 rounded-md px-1 py-1"
              >
                <StatusIcon status={item.status} />
                <span className="truncate text-sm">{item.file.name}</span>
                {item.status === 'pending' && !uploading ? (
                  <button
                    type="button"
                    aria-label={`Remove ${item.file.name}`}
                    className="ml-auto rounded p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    onClick={() => removeItem(item.id)}
                  >
                    <XIcon className="size-3.5" />
                  </button>
                ) : null}
              </li>
            ))}
          </ul>

          {(pending.length > 0 || uploading) && !finished ? (
            <Button
              type="button"
              variant="default"
              size="lg"
              className="mt-3 w-full"
              data-testid="upload-button"
              onClick={doUpload}
              disabled={uploading || pending.length === 0}
            >
              {uploading ? (
                <LoaderCircleIcon className="animate-spin" />
              ) : (
                <UploadCloudIcon />
              )}
              {uploading
                ? 'Uploading...'
                : `Upload ${pending.length} file${pending.length === 1 ? '' : 's'}`}
            </Button>
          ) : null}

          {finished ? (
            failed.length === 0 ? (
              <div
                data-testid="upload-success"
                className="mt-3 rounded-md border border-emerald-500/30 bg-emerald-500/5 px-2.5 py-1.5 text-xs text-emerald-600 dark:text-emerald-400"
              >
                Uploaded {done.length} file{done.length === 1 ? '' : 's'}.
              </div>
            ) : (
              <div
                data-testid="upload-error"
                className="mt-3 rounded-md border border-destructive/30 bg-destructive/5 px-2.5 py-1.5 text-xs text-destructive"
              >
                Uploaded {done.length}, {failed.length} failed.
              </div>
            )
          ) : null}
        </div>
      ) : null}
    </div>
  )
}
