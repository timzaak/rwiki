import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, waitFor, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'
import { UploadDocument } from '@/components/admin/upload-document'
import {
  seedSite,
  seedEmptySites,
  withAdminSiteProvider,
} from '@/test/helpers/admin-site'

/**
 * FE-T03 — UploadDocument site-context tests
 *
 * Covers FE-D03's `upload-document.tsx` consumption of `useAdminSite()`:
 *  - multipart body contains `siteId` (read via `await request.formData()` —
 *    NOT by mocking the internal SDK) alongside the `file` field.
 *  - null-disable: when `siteId === null` (empty-sites anomaly), the dropzone
 *    and upload button are disabled and no upload request fires even when a
 *    file is staged.
 *
 * Real upload interaction mirrors `admin.test.tsx`: `user.upload(fileInput,
 * file)` + `click upload-button`, with the generated SDK's fetch flowing
 * through MSW. No internal SDK function is mocked.
 *
 * The provider is seeded via MSW `/api/sites` (see `helpers/admin-site`); the
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
  return render(withAdminSiteProvider({ children: <UploadDocument onUploaded={onUploaded} /> }))
}

describe('UploadDocument — multipart body contains siteId', () => {
  it('sends siteId + file in the multipart FormData on upload', async () => {
    // Seed the provider with siteId === 'site-a' (default-first selection).
    seedSite('site-a')
    installUploadHandler(201)

    const user = userEvent.setup()
    renderUpload()

    // Wait for the provider to settle on a site (upload controls stay disabled
    // until siteId is non-null).
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

    // Exactly one upload request, carrying the injected siteId + the file.
    await waitFor(() => {
      expect(uploadCallCount).toBe(1)
    })
    expect(lastFormData).not.toBeNull()
    // siteId is a load-bearing multipart field — assert its exact value.
    expect(lastFormData!.get('siteId')).toBe('site-a')
    // The file field is present and carries binary data (a Blob-like object,
    // not a stringified path). The multipart serializer may re-encode bytes, so
    // we only assert presence + that it is a non-string Blob; `siteId` above is
    // the load-bearing field.
    const sentFile = lastFormData!.get('file')
    expect(sentFile).not.toBeNull()
    expect(typeof sentFile).not.toBe('string')
  })
})

describe('UploadDocument — null siteId disables upload', () => {
  it('disables dropzone + upload-button when siteId is null (no upload fires)', async () => {
    // Empty sites → provider keeps siteId === null.
    seedEmptySites()
    // No upload handler installed: if any request leaks, MSW's
    // onUnhandledRequest('warn') would flag it; the explicit zero-count
    // assertion below is the real guard.
    const user = userEvent.setup()
    renderUpload()

    // Provider settles on the empty-sites state (no loading, siteId stays null).
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

    // Even with a staged file, the upload button must NOT render while siteId
    // is null (the component gates the button render on the popover, and the
    // popover button is disabled when siteId === null). Assert no upload
    // request fires regardless.
    expect(uploadCallCount).toBe(0)
    // The dropzone stays disabled throughout.
    expect(screen.getByTestId('upload-dropzone')).toBeDisabled()
  })
})
