/**
 * Document Management API Tests (support-multiple-website 迁移)
 *
 * The document lifecycle API now requires `channelId` on every endpoint and
 * returns 400 without it. This file exercises the full document lifecycle
 * against the `help_center` demo channel, plus cross-channel isolation and per-channel
 * config isolation against the intentionally-empty `developer_docs` channel.
 *
 * All operations are direct HTTP API calls — no UI interaction, no demoLogger
 * fixture (API-only, matching this file's existing import style).
 *
 * US-story traceability:
 * - US-CORE-001 (legacy, `docs/user-stories/...`): upload xlsx via API → verify response.
 * - US-CORE-004 (legacy, `docs/user-stories/...`): list documents via API → delete via API.
 * - US-CORE-009 (legacy, `docs/user-stories/...`): publish / unpublish draft document.
 * - US-INTG-008 scn 1 (DRAFT, `.ai/user-stories/integration/support-multiple-website.md`):
 *     cross-channel document isolation — a doc uploaded to `help_center` is
 *     absent from `developer_docs`, and a cross-channel lifecycle op returns 404
 *     per design §4.2.2 (cross-channel → 404).
 * - US-INTG-008 scn 3-proxy (DRAFT, same source): system-prompt isolation is
 *     proxied at the API level via per-channel suggested-questions isolation
 *     (`help_center` returns configured questions, `developer_docs` returns
 *     `[]`). The prompt-driven chat answer style is intentionally NOT asserted
 *     here (LLM-flaky); the suggestions-per-channel API assertion is the
 *     documented proxy.
 *
 * Source path note: the US-INTG-008 references above are to a DRAFT user-story
 * file under `.ai/user-stories/...` (pre-publish Demo) and must NOT be
 * rewritten as published fact.
 */

import { test, expect } from '@playwright/test'
import { TEST_XLSX_PATH } from './fixtures/test-xlsx'
import fs from 'node:fs'

// Demo backend (port 18080). NOT the old 8080.
const BASE_URL = process.env.BASE_URL || 'http://localhost:18080'
const HELP_CENTER = 'help_center'
const DEVELOPER_DOCS = 'developer_docs'
const authHeaders = { Authorization: 'Bearer demo-token' }

let createdDocumentIds: string[] = []

test.describe('Document Management API', () => {
  test.beforeEach(() => {
    createdDocumentIds = []
  })

  test.afterEach(async ({ request }) => {
    for (const docId of createdDocumentIds.splice(0)) {
      await request
        .delete(`${BASE_URL}/api/documents/${docId}?channelId=${HELP_CENTER}`, {
          headers: authHeaders,
        })
        .catch(() => {})
    }
  })

  // US-CORE-001: scenario 1 - Upload valid xlsx file and verify response
  test('US-CORE-001 scenario 1 - upload valid xlsx file and verify response', async ({
    request,
  }) => {
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)

    const response = await request.post(`${BASE_URL}/api/documents/upload`, {
      headers: authHeaders,
      multipart: {
        channelId: HELP_CENTER,
        file: {
          name: 'test-data.xlsx',
          mimeType:
            'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
          buffer: fileBuffer,
        },
      },
    })

    expect(response.ok(), `Upload should succeed, got ${response.status()}`).toBeTruthy()
    const body = await response.json()

    expect(body.id).toBeTruthy()
    expect(body.fileName).toBeTruthy()
    expect(['draft', 'indexed', 'processing']).toContain(body.status)
    // channelId is now returned on the upload response body (design §4.2.2 / §5.5).
    expect(body.channelId).toBe(HELP_CENTER)

    createdDocumentIds.push(body.id)
  })

  // US-CORE-001: scenario 2 - Upload rejects invalid format
  test('US-CORE-001 scenario 2 - upload rejects invalid format', async ({
    request,
  }) => {
    const response = await request.post(`${BASE_URL}/api/documents/upload`, {
      headers: authHeaders,
      multipart: {
        channelId: HELP_CENTER,
        file: {
          name: 'invalid.txt',
          mimeType: 'text/plain',
          buffer: Buffer.from('not an xlsx'),
        },
      },
    })

    expect(response.ok()).toBeFalsy()
    expect(response.status()).toBe(400)
  })

  // US-CORE-004: scenario 1 - List documents
  test('US-CORE-004 scenario 1 - list documents via API', async ({
    request,
  }) => {
    // Ensure at least one document exists in help_center
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
          channelId: HELP_CENTER,
          file: {
            name: 'test-data.xlsx',
            mimeType:
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            buffer: fileBuffer,
          },
        },
      }
    )
    expect(uploadResp.ok()).toBeTruthy()
    const uploadBody = await uploadResp.json()
    createdDocumentIds.push(uploadBody.id)

    // List documents scoped to help_center
    const listResp = await request.get(
      `${BASE_URL}/api/documents?channelId=${HELP_CENTER}`,
      { headers: authHeaders }
    )
    expect(listResp.ok()).toBeTruthy()
    const listBody = await listResp.json()

    expect(Array.isArray(listBody.documents)).toBeTruthy()
    expect(listBody.documents.length).toBeGreaterThan(0)

    // All returned docs must belong to help_center (channel scoping contract).
    for (const doc of listBody.documents) {
      expect(doc.channelId).toBe(HELP_CENTER)
    }

    const doc = listBody.documents.find(
      (d: { id: string }) => d.id === uploadBody.id
    )
    expect(doc).toBeTruthy()
    expect(doc.fileName).toBeTruthy()
    expect(doc.status).toBeTruthy()
  })

  // US-CORE-004: scenario 2 - Delete document
  test('US-CORE-004 scenario 2 - delete document via API', async ({
    request,
  }) => {
    // Upload a document first
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
          channelId: HELP_CENTER,
          file: {
            name: 'test-data.xlsx',
            mimeType:
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            buffer: fileBuffer,
          },
        },
      }
    )
    expect(uploadResp.ok()).toBeTruthy()
    const uploadBody = await uploadResp.json()
    const docId = uploadBody.id

    // Delete the document scoped to help_center
    const deleteResp = await request.delete(
      `${BASE_URL}/api/documents/${docId}?channelId=${HELP_CENTER}`,
      { headers: authHeaders }
    )
    expect(deleteResp.ok()).toBeTruthy()

    // Verify it's gone from the help_center list
    const listResp = await request.get(
      `${BASE_URL}/api/documents?channelId=${HELP_CENTER}`,
      { headers: authHeaders }
    )
    expect(listResp.ok()).toBeTruthy()
    const listBody = await listResp.json()
    const found = listBody.documents.find(
      (d: { id: string }) => d.id === docId
    )
    expect(found).toBeFalsy()
  })

  // US-CORE-009: scenario 1 - Publish draft document
  test('US-CORE-009 scenario 1 - publish draft document', async ({
    request,
  }) => {
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
          channelId: HELP_CENTER,
          file: {
            name: 'test-data.xlsx',
            mimeType:
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            buffer: fileBuffer,
          },
        },
      }
    )
    expect(uploadResp.ok()).toBeTruthy()
    const uploadBody = await uploadResp.json()
    const docId = uploadBody.id
    createdDocumentIds.push(docId)

    // Wait for indexing to complete (status should become draft)
    const status = await waitForDocumentStatus(request, docId, [
      'draft',
      'published',
    ])
    expect(['draft', 'published']).toContain(status)

    if (status === 'draft') {
      const publishResp = await request.patch(
        `${BASE_URL}/api/documents/${docId}/publish?channelId=${HELP_CENTER}`,
        { headers: authHeaders, data: '' }
      )
      expect(publishResp.ok()).toBeTruthy()
      const publishBody = await publishResp.json()
      expect(publishBody.status).toBe('published')
    }
  })

  // US-CORE-009: scenario 2 - Unpublish document
  test('US-CORE-009 scenario 2 - unpublish document', async ({
    request,
  }) => {
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
          channelId: HELP_CENTER,
          file: {
            name: 'test-data.xlsx',
            mimeType:
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            buffer: fileBuffer,
          },
        },
      }
    )
    expect(uploadResp.ok()).toBeTruthy()
    const uploadBody = await uploadResp.json()
    const docId = uploadBody.id
    createdDocumentIds.push(docId)

    // Ensure document is published first
    const status = await waitForDocumentStatus(request, docId, [
      'draft',
      'published',
    ])
    if (status === 'draft') {
      await request.patch(
        `${BASE_URL}/api/documents/${docId}/publish?channelId=${HELP_CENTER}`,
        { headers: authHeaders, data: '' }
      )
    }

    // Unpublish scoped to help_center
    const unpublishResp = await request.patch(
      `${BASE_URL}/api/documents/${docId}/unpublish?channelId=${HELP_CENTER}`,
      { headers: authHeaders, data: '' }
    )
    expect(unpublishResp.ok()).toBeTruthy()
    const unpublishBody = await unpublishResp.json()
    expect(unpublishBody.status).toBe('draft')
  })

  // -------------------------------------------------------------------------
  // US-INTG-008 scn 1 - cross-channel document isolation.
  // A doc uploaded+published into help_center must be absent from
  // developer_docs, and a cross-channel lifecycle op (DELETE with the wrong
  // channelId) returns 404 per design §4.2.2 without deleting the doc.
  // -------------------------------------------------------------------------
  test('US-INTG-008 scenario 1 - cross-channel document isolation', async ({
    request,
  }) => {
    // Upload + publish doc《A》 into help_center.
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
          channelId: HELP_CENTER,
          file: {
            name: 'test-data.xlsx',
            mimeType:
              'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
            buffer: fileBuffer,
          },
        },
      }
    )
    expect(uploadResp.ok(), `Upload to ${HELP_CENTER} failed: ${uploadResp.status()}`).toBeTruthy()
    const uploadBody = await uploadResp.json()
    const helpCenterDocId = uploadBody.id
    createdDocumentIds.push(helpCenterDocId)

    // Publish doc《A》 into help_center (poll to draft first, per the
    // chat-rag-streaming pattern — publish requires status='draft').
    await waitForDocumentStatus(request, helpCenterDocId, ['draft', 'published'])
    await request.patch(
      `${BASE_URL}/api/documents/${helpCenterDocId}/publish?channelId=${HELP_CENTER}`,
      { headers: authHeaders, data: '' }
    )

    // Assert doc《A》 is absent from developer_docs.
    const devDocsListResp = await request.get(
      `${BASE_URL}/api/documents?channelId=${DEVELOPER_DOCS}`,
      { headers: authHeaders }
    )
    expect(devDocsListResp.ok()).toBeTruthy()
    const devDocsListBody = await devDocsListResp.json()
    expect(Array.isArray(devDocsListBody.documents)).toBeTruthy()
    const leakedIntoDevDocs = devDocsListBody.documents.find(
      (d: { id: string }) => d.id === helpCenterDocId
    )
    expect(
      leakedIntoDevDocs,
      `doc《A》(${helpCenterDocId}) must not be visible in ${DEVELOPER_DOCS}`
    ).toBeFalsy()

    // Cross-channel lifecycle op: DELETE with the WRONG channelId must return 404
    // (design §4.2.2 cross-channel → 404) and must NOT delete the doc.
    const crossChannelDeleteResp = await request.delete(
      `${BASE_URL}/api/documents/${helpCenterDocId}?channelId=${DEVELOPER_DOCS}`,
      { headers: authHeaders }
    )
    expect(crossChannelDeleteResp.status()).toBe(404)

    // Verify doc《A》 is still present in help_center (the cross-channel delete
    // must not have deleted it).
    const helpCenterListResp = await request.get(
      `${BASE_URL}/api/documents?channelId=${HELP_CENTER}`,
      { headers: authHeaders }
    )
    expect(helpCenterListResp.ok()).toBeTruthy()
    const helpCenterListBody = await helpCenterListResp.json()
    const stillThere = helpCenterListBody.documents.find(
      (d: { id: string }) => d.id === helpCenterDocId
    )
    expect(stillThere, 'doc《A》 must still exist in help_center after a cross-channel delete attempt').toBeTruthy()
  })

  // -------------------------------------------------------------------------
  // US-INTG-008 scn 3 proxy - per-channel config isolation via suggestions API.
  //
  // system-prompt isolation (scn 3) is LLM-answer-style-flaky if asserted via
  // chat output, so it is proxied here at the API level: per-channel suggested
  // questions isolation proves per-channel config isolation without invoking the
  // LLM. help_center has configured zh-CN questions; developer_docs has NO
  // suggested_questions (empty-array contract) and must return [].
  // -------------------------------------------------------------------------
  test('US-INTG-008 scenario 3 proxy - per-channel suggestions isolation', async ({
    request,
  }) => {
    // help_center has configured suggested_questions for zh-CN.
    const helpCenterResp = await request.get(
      `${BASE_URL}/api/chat/suggestions?channelId=${HELP_CENTER}&locale=zh-CN`,
      { headers: authHeaders }
    )
    expect(helpCenterResp.ok(), `help_center suggestions failed: ${helpCenterResp.status()}`).toBeTruthy()
    const helpCenterBody = await helpCenterResp.json()
    expect(Array.isArray(helpCenterBody.questions)).toBeTruthy()
    expect(
      helpCenterBody.questions.length,
      'help_center should return configured zh-CN suggested questions'
    ).toBeGreaterThan(0)

    // developer_docs has NO suggested_questions configured → must return [].
    const devDocsResp = await request.get(
      `${BASE_URL}/api/chat/suggestions?channelId=${DEVELOPER_DOCS}&locale=zh-CN`,
      { headers: authHeaders }
    )
    expect(devDocsResp.ok(), `developer_docs suggestions failed: ${devDocsResp.status()}`).toBeTruthy()
    const devDocsBody = await devDocsResp.json()
    expect(Array.isArray(devDocsBody.questions)).toBeTruthy()
    expect(
      devDocsBody.questions,
      'developer_docs (no configured questions) must return an empty array'
    ).toEqual([])
  })
})

/**
 * Poll a channel-scoped document's status until it reaches one of the target
 * statuses. The list call always carries `?channelId=help_center`.
 * Returns the final status string.
 */
async function waitForDocumentStatus(
  request: import('@playwright/test').APIRequestContext,
  docId: string,
  targetStatuses: string[],
  timeoutMs: number = 30000,
): Promise<string> {
  const start = Date.now()
  while (Date.now() - start < timeoutMs) {
    const resp = await request.get(
      `${BASE_URL}/api/documents?channelId=${HELP_CENTER}`,
      { headers: authHeaders }
    )
    if (resp.ok()) {
      const body = await resp.json()
      const doc = body.documents?.find(
        (d: { id: string }) => d.id === docId
      )
      if (doc && targetStatuses.includes(doc.status)) {
        return doc.status
      }
    }
    await new Promise((r) => setTimeout(r, 1000))
  }
  throw new Error(
    `Document ${docId} did not reach status ${targetStatuses.join('/')} within ${timeoutMs}ms`
  )
}
