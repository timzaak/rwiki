import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { http, HttpResponse } from 'msw'

import { server } from '@/test/mocks/server'

/**
 * FE-T01 — api-client-setup interceptor tests
 *
 * Covers FE-D01's `installApiClientAuth()`:
 *  - Bearer injected when localStorage key present; absent when not.
 *  - 401 response → clearApiKey + redirect to /auth/login (unless already there).
 *  - Non-401 (200/204/500) and network errors do NOT clear key or redirect.
 *
 * The production module auto-runs `installApiClientAuth()` on import and guards
 * it with an `installed` singleton; the generated `client` accumulates error
 * interceptors across calls. To get a clean client + fresh interceptors per
 * test, we `vi.resetModules()` and dynamically re-import both the setup module
 * and the SDK, then drive the SDK's `verifyToken()` (GET /api/auth/verify)
 * through MSW.
 */

const BASE_URL = 'http://localhost:3000'
const API_KEY = 'test-key'
const KEY_STORAGE = 'rwiki_api_key'

// Default MSW handler captures the outgoing Authorization header into a closure
// variable so each test can assert exactly what the SDK sent.
let capturedAuth: string | null | undefined = undefined

function defaultVerifyHandler() {
  return http.get(`${BASE_URL}/api/auth/verify`, ({ request }) => {
    capturedAuth = request.headers.get('Authorization')
    return new HttpResponse(null, { status: 204 })
  })
}

// --- window.location stub -------------------------------------------------
// jsdom treats `window.location.href = ...` as a navigation it cannot perform,
// emitting a noisy "Not implemented" error and leaving pathname unchanged.
// We install a minimal, controllable stub so the interceptor's
// `pathname.startsWith('/auth/login')` branch and `href = '/auth/login'`
// assignment can be asserted deterministically. Restored in afterAll.
interface LocationStub {
  pathname: string
  href: string
}

const originalLocation = window.location

function installLocationStub(initialPathname: string): LocationStub {
  const stub: LocationStub = {
    pathname: initialPathname,
    href: initialPathname,
  }
  Object.defineProperty(window, 'location', {
    value: stub,
    writable: true,
    configurable: true,
  })
  return stub
}

// --- per-test fresh module graph -----------------------------------------
// `verifyToken` is reassigned in each beforeEach via dynamic import after a
// module reset; typed against the generated SDK function signature.
type VerifyTokenFn = typeof import('@/lib/api-generated/sdk.gen')['verifyToken']
let verifyToken: VerifyTokenFn

async function freshInstall() {
  // Re-import the generated client + setup module + SDK so the `installed`
  // singleton resets and interceptors start empty for each test.
  const { client } = await import('@/lib/api-generated/client.gen')
  const { installApiClientAuth } = await import('@/lib/api-client-setup')
  const sdk = await import('@/lib/api-generated/sdk.gen')

  // jsdom requires an absolute URL for fetch to hit MSW; the production
  // installer only sets `auth`, so we layer baseUrl on top (config merges).
  client.setConfig({ baseUrl: BASE_URL })
  installApiClientAuth()

  verifyToken = sdk.verifyToken
}

describe('installApiClientAuth — Bearer injection', () => {
  let locationStub: LocationStub

  beforeEach(async () => {
    localStorage.clear()
    capturedAuth = undefined
    locationStub = installLocationStub('/admin')
    server.use(defaultVerifyHandler())
    await freshInstall()
  })

  afterEach(() => {
    vi.resetModules()
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      configurable: true,
      writable: true,
    })
  })

  it('injects Authorization: Bearer <key> when a key is stored', async () => {
    localStorage.setItem(KEY_STORAGE, API_KEY)

    await verifyToken()

    expect(capturedAuth).toBe(`Bearer ${API_KEY}`)
  })

  it('does not inject Authorization when no key is stored', async () => {
    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()

    await verifyToken()

    expect(capturedAuth).toBeNull()
    // confirm we never redirected on a successful probe
    expect(locationStub.href).toBe('/admin')
  })
})

describe('installApiClientAuth — 401 handling', () => {
  let locationStub: LocationStub

  beforeEach(async () => {
    localStorage.clear()
    capturedAuth = undefined
    locationStub = installLocationStub('/admin')
    await freshInstall()
  })

  afterEach(() => {
    vi.resetModules()
    Object.defineProperty(window, 'location', {
      value: originalLocation,
      configurable: true,
      writable: true,
    })
  })

  it('clears the key and redirects to /auth/login on 401 when not on login page', async () => {
    localStorage.setItem(KEY_STORAGE, API_KEY)
    server.use(
      http.get(`${BASE_URL}/api/auth/verify`, () =>
        HttpResponse.json({ message: 'Unauthorized' }, { status: 401 }),
      ),
    )

    await verifyToken()

    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()
    expect(locationStub.href).toBe('/auth/login')
  })

  it('clears the key but does NOT redirect on 401 when already on /auth/login', async () => {
    localStorage.setItem(KEY_STORAGE, API_KEY)
    locationStub.pathname = '/auth/login'
    locationStub.href = '/auth/login'
    server.use(
      http.get(`${BASE_URL}/api/auth/verify`, () =>
        HttpResponse.json({ message: 'Unauthorized' }, { status: 401 }),
      ),
    )

    await verifyToken()

    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()
    // already on login page — must not navigate (avoids redirect loop)
    expect(locationStub.href).toBe('/auth/login')
  })

  it.each([
    { status: 200, label: '200 OK' },
    { status: 204, label: '204 No Content' },
    { status: 500, label: '500 Server Error' },
  ])(
    'does NOT clear the key or redirect on non-401 ($label)',
    async ({ status }) => {
      localStorage.setItem(KEY_STORAGE, API_KEY)
      server.use(
        http.get(`${BASE_URL}/api/auth/verify`, () =>
          HttpResponse.json({ message: 'ok' }, { status }),
        ),
      )

      await verifyToken()

      expect(localStorage.getItem(KEY_STORAGE)).toBe(API_KEY)
      expect(locationStub.href).toBe('/admin')
    },
  )

  it('does NOT clear the key or redirect on a network error (no response)', async () => {
    localStorage.setItem(KEY_STORAGE, API_KEY)
    // Force the SDK's underlying fetch to reject — the client routes this
    // through the error interceptor with `response === undefined`, which the
    // interceptor must not treat as a 401.
    server.use(
      http.get(`${BASE_URL}/api/auth/verify`, () =>
        HttpResponse.error(),
      ),
    )

    await verifyToken()

    // Key preserved: a transient network failure must not log the user out.
    expect(localStorage.getItem(KEY_STORAGE)).toBe(API_KEY)
    expect(locationStub.href).toBe('/admin')
  })
})
