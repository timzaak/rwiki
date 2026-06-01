/**
 * Document Management API Tests
 *
 * Covers:
 * - US-CORE-001: Upload xlsx file via API -> verify response
 * - US-CORE-004: List documents via API -> delete document via API
 *
 * All operations use direct HTTP API calls — no UI interaction.
 */

import { test, expect } from '@playwright/test'
import { TEST_XLSX_PATH } from './fixtures/test-xlsx'
import fs from 'node:fs'

const BASE_URL = process.env.BASE_URL || 'http://localhost:8080'
const authHeaders = { Authorization: 'Bearer demo-token' }

let createdDocumentIds: string[] = []

test.describe('Document Management API', () => {
  test.beforeEach(() => {
    createdDocumentIds = []
  })

  test.afterEach(async ({ request }) => {
    for (const docId of createdDocumentIds.splice(0)) {
      await request
        .delete(`${BASE_URL}/api/documents/${docId}`, {
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

    createdDocumentIds.push(body.id)
  })

  // US-CORE-001: scenario 2 - Upload rejects invalid format
  test('US-CORE-001 scenario 2 - upload rejects invalid format', async ({
    request,
  }) => {
    const response = await request.post(`${BASE_URL}/api/documents/upload`, {
      headers: authHeaders,
      multipart: {
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
    // Ensure at least one document exists
    const fileBuffer = fs.readFileSync(TEST_XLSX_PATH)
    const uploadResp = await request.post(
      `${BASE_URL}/api/documents/upload`,
      {
        headers: authHeaders,
        multipart: {
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

    // List documents
    const listResp = await request.get(`${BASE_URL}/api/documents`, {
      headers: authHeaders,
    })
    expect(listResp.ok()).toBeTruthy()
    const listBody = await listResp.json()

    expect(Array.isArray(listBody.documents)).toBeTruthy()
    expect(listBody.documents.length).toBeGreaterThan(0)

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

    // Delete the document
    const deleteResp = await request.delete(
      `${BASE_URL}/api/documents/${docId}`,
      { headers: authHeaders }
    )
    expect(deleteResp.ok()).toBeTruthy()

    // Verify it's gone from the list
    const listResp = await request.get(`${BASE_URL}/api/documents`, {
      headers: authHeaders,
    })
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
        `${BASE_URL}/api/documents/${docId}/publish`,
        { headers: authHeaders }
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
      await request.patch(`${BASE_URL}/api/documents/${docId}/publish`, {
        headers: authHeaders,
      })
    }

    // Unpublish
    const unpublishResp = await request.patch(
      `${BASE_URL}/api/documents/${docId}/unpublish`,
      { headers: authHeaders }
    )
    expect(unpublishResp.ok()).toBeTruthy()
    const unpublishBody = await unpublishResp.json()
    expect(unpublishBody.status).toBe('draft')
  })
})

/**
 * Poll document status until it reaches one of the target statuses.
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
    const resp = await request.get(`${BASE_URL}/api/documents`, {
      headers: authHeaders,
    })
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
