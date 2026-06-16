import { useState } from 'react'
import { LoaderCircleIcon, UploadCloudIcon } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { uploadDocument } from '@/lib/api-generated/sdk.gen'

export interface UploadDocumentProps {
  /** Called after a successful upload so the parent can refresh the list. */
  onUploaded: () => void
}

export function UploadDocument({ onUploaded }: UploadDocumentProps) {
  const [file, setFile] = useState<File | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState(false)

  function selectFile(next: File | null) {
    setFile(next)
    setError(null)
    setSuccess(false)
  }

  async function doUpload() {
    if (!file) return
    setLoading(true)
    setError(null)
    setSuccess(false)
    try {
      // multipart serialization is handled by the generated SDK
      // (formDataBodySerializer + Content-Type cleared); no manual work here.
      const result = await uploadDocument({ body: { file } })
      if (result.error || (result.response && !result.response.ok)) {
        setError('上传失败')
      } else {
        setSuccess(true)
        setFile(null)
        onUploaded()
      }
    } catch {
      setError('上传失败')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div
      data-testid="upload-document"
      className="flex flex-col gap-3 rounded-lg border border-border/60 bg-card p-4"
    >
      <div
        data-testid="upload-dropzone"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => {
          e.preventDefault()
          const dropped = e.dataTransfer.files?.[0]
          if (dropped) selectFile(dropped)
        }}
        className="flex flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border/60 px-4 py-8 text-center text-sm text-muted-foreground"
      >
        <UploadCloudIcon className="size-5" />
        <span>Drag a file here, or select one below.</span>
        {/* 内联 label（非 Button）：这是一个包裹隐藏 file input 的 <label>，
            Button 渲染 <button>，语义上无法承载 file input，故保留内联样式。 */}
        <label className="mt-1 inline-flex h-8 cursor-pointer items-center rounded-lg bg-primary px-3 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90">
          Select file
          <input
            type="file"
            data-testid="file-input"
            className="hidden"
            onChange={(e) => selectFile(e.target.files?.[0] ?? null)}
          />
        </label>
        {file ? (
          <span className="text-xs text-foreground">{file.name}</span>
        ) : null}
      </div>

      {error ? (
        <div
          data-testid="upload-error"
          className="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive"
        >
          {error}
        </div>
      ) : null}

      {success && !error ? (
        <div
          data-testid="upload-success"
          className="rounded-lg border border-emerald-500/30 bg-emerald-500/5 px-3 py-2 text-sm text-emerald-600 dark:text-emerald-400"
        >
          Upload succeeded.
        </div>
      ) : null}

      <Button
        type="button"
        variant="default"
        size="lg"
        data-testid="upload-button"
        onClick={doUpload}
        disabled={!file || loading}
      >
        {loading ? (
          <LoaderCircleIcon className="size-4 animate-spin" />
        ) : (
          <UploadCloudIcon className="size-4" />
        )}
        {loading ? 'Uploading...' : 'Upload'}
      </Button>
    </div>
  )
}
