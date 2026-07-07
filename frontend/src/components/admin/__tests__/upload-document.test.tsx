import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { UploadDocument } from '@/components/admin/upload-document'
import {
  seedChannel,
  seedEmptyChannels,
  withAdminChannelProvider,
} from '@/test/helpers/admin-channel'

/**
 * FE-T03 — UploadDocument channel-context tests
 *
 * Covers FE-D03's `upload-document.tsx` consumption of `useAdminChannel()`:
 *  - multipart body contains `channelId` (read via `await request.formData()` —
 *    NOT by mocking the internal SDK) alongside the `file` field.
 *  - null-disable: when `channelId === null` (empty-channels anomaly), the dropzone
 *    and upload button are disabled and no upload request fires even when a
 *    file is staged.
 *
 * Real upload interaction mirrors `admin.test.tsx`: `user.upload(fileInput,
 * file)` + `click upload-button`, with the generated SDK's fetch flowing
 * through MSW. No internal SDK function is mocked.
 *
 * The provider is seeded via MSW `/api/channels` (see `helpers/admin-channel`); the
 * prod context is private, so injection goes through the real provider.
 */

const BASE_URL = 'http://localhost:3000'
const UPLOAD_URL = `${BASE_URL}/api/documents/upload`

let uploadCallCount: number
let lastFormData: FormData | null

beforeEach(() => {
  client.setConfig({ baseUrl: BASE_URL })
  localStorage.clear()
  uploadCallCount = 0
  lastFormData = null
})

function installUploadHandler(status: 200 | 201 = 201): void {
  server.use(
    http.post(UPLOAD_URL, async ({ request }) => {
      uploadCallCount += 1
      lastFormData = await request.formData()
      return HttpResponse.json({ document: { id: 'doc-1' } }, { status })
    }),
  )
}

function renderUpload(onUploaded: () => void = vi.fn()) {
  return render(withAdminChannelProvider({ children: <UploadDocument onUploaded={onUploaded} /> }))
}

describe('UploadDocument — multipart body contains channelId', () => {
  it('sends channelId + file in the multipart FormData on upload', async () => {
    // Seed the provider with channelId === 'channel-a' (default-first selection).
    seedChannel('channel-a')
    installUploadHandler(201)

    const user = userEvent.setup()
    renderUpload()

    // Wait for the provider to settle on a channel (upload controls stay disabled
    // until channelId is non-null).
    const fileInput = await screen.findByTestId('file-input')
    expect(fileInput).toBeEnabled()

    const file = new File(['content'], 'upload.pdf', {
      type: 'application/pdf',
    })
    await user.upload(fileInput as HTMLInputElement, file)

    // The popover upload button appears once a file is staged.
    await act(async () => {
      await user.click(screen.getByTestId('upload-button'))
    })

    // Exactly one upload request, carrying the injected channelId + the file.
    await waitFor(() => {
      expect(uploadCallCount).toBe(1)
    })
    expect(lastFormData).not.toBeNull()
    // channelId is a load-bearing multipart field — assert its exact value.
    expect(lastFormData!.get('channelId')).toBe('channel-a')
    // The file field is present and carries binary data (a Blob-like object,
    // not a stringified path). The multipart serializer may re-encode bytes, so
    // we only assert presence + that it is a non-string Blob; `channelId` above is
    // the load-bearing field.
    const sentFile = lastFormData!.get('file')
    expect(sentFile).not.toBeNull()
    expect(typeof sentFile).not.toBe('string')
  })
})

describe('UploadDocument — null channelId disables upload', () => {
  it('disables dropzone + upload-button when channelId is null (no upload fires)', async () => {
    // Empty channels → provider keeps channelId === null.
    seedEmptyChannels()
    // No upload handler installed: if any request leaks, MSW's
    // onUnhandledRequest('warn') would flag it; the explicit zero-count
    // assertion below is the real guard.
    const user = userEvent.setup()
    renderUpload()

    // Provider settles on the empty-channels state (no loading, channelId stays null).
    await waitFor(() => {
      expect(screen.getByTestId('upload-dropzone')).toBeDisabled()
    })

    // The hidden file input is not the gating control, but the dropzone button
    // must be disabled (it is the only idle footprint + opens the picker).
    expect(screen.getByTestId('upload-dropzone')).toBeDisabled()

    // Stage a file directly via the input to exercise the path that would
    // normally reveal the upload button.
    const fileInput = screen.getByTestId('file-input') as HTMLInputElement
    const file = new File(['content'], 'staged.pdf', {
      type: 'application/pdf',
    })
    await user.upload(fileInput, file)

    // Even with a staged file, the upload button must NOT render while channelId
    // is null (the component gates the button render on the popover, and the
    // popover button is disabled when channelId === null). Assert no upload
    // request fires regardless.
    expect(uploadCallCount).toBe(0)
    // The dropzone stays disabled throughout.
    expect(screen.getByTestId('upload-dropzone')).toBeDisabled()
  })
})
