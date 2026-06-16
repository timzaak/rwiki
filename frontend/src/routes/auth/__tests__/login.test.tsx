import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { isRedirect } from '@tanstack/router-core'
import type { ComponentType } from 'react'

import { server } from '@/test/mocks/server'
import { client } from '@/lib/api-generated/client.gen'

/**
 * FE-T02 — login page probe tests
 *
 * Covers FE-D02's `LoginComponent` (`src/routes/auth/login.tsx`):
 *  - 204 probe → `setApiKey(key)` persisted + navigate('/admin').
 *  - 401 / non-204 / network error → `error-message` shown, key NOT stored, no navigation.
 *  - submit disabled while probing (loading state).
 *  - `beforeLoad` guard redirects already-authenticated users to /admin.
 *
 * The route uses TanStack Router's `useNavigate()` and `createFileRoute`. We mock
 * `@tanstack/react-router` to substitute `useNavigate` with a spy while keeping
 * `createFileRoute` and `redirect` real — the former so `Route.beforeLoad` is
 * reachable as a plain function, the latter so the thrown redirect value is
 * assertable via `isRedirect()` from `@tanstack/router-core`.
 *
 * `login.tsx` does not import `api-client-setup`, so FE-D01's global 401
 * interceptor is NOT installed in this module graph; the component's own
 * non-204 branch is the sole failure path under test here.
 */

const BASE_URL = 'http://localhost:3000'
const VERIFY_URL = `${BASE_URL}/api/auth/verify`
const KEY_STORAGE = 'rwiki_api_key'
const CANDIDATE_KEY = 'candidate-key-abc'

// navigate spy is reassigned in each beforeEach via the factory below so each
// test gets a fresh mock without re-running vi.mock.
const navigateSpy = vi.fn()

vi.mock('@tanstack/react-router', async (orig) => {
  const actual: Record<string, unknown> = await orig()
  return {
    ...actual,
    // Substitute only the hook used inside LoginComponent; createFileRoute and
    // redirect stay real so the route module loads and beforeLoad is callable.
    useNavigate: () => navigateSpy,
  }
})

// Import AFTER vi.mock so the route module picks up the mocked useNavigate.
const { Route } = await import('@/routes/auth/login')

// The generated Route types `options.component` as `unknown`; cast once to the
// shape we render with. `beforeLoad`'s ctx is unused by the implementation
// (it only reads `isAuthenticated()`), so we type the reference loosely.
const LoginComponent = Route.options.component as ComponentType
type BeforeLoadFn = (ctx: unknown) => unknown
const beforeLoad = Route.options.beforeLoad as BeforeLoadFn | undefined

describe('LoginComponent — probe submit', () => {
  beforeEach(() => {
    localStorage.clear()
    navigateSpy.mockReset()
    // jsdom needs an absolute URL for fetch to reach MSW; layer baseUrl on top
    // of whatever the generated client already configured (config merges).
    client.setConfig({ baseUrl: BASE_URL })
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('stores the key and navigates to /admin on a valid probe (204)', async () => {
    server.use(
      http.get(VERIFY_URL, () => new HttpResponse(null, { status: 204 })),
    )
    const user = userEvent.setup()
    render(<LoginComponent />)

    await user.type(screen.getByTestId('api-key-input'), CANDIDATE_KEY)
    await user.click(screen.getByTestId('submit-button'))

    // Key persisted to localStorage + navigation to /admin triggered.
    await waitFor(() => {
      expect(localStorage.getItem(KEY_STORAGE)).toBe(CANDIDATE_KEY)
    })
    expect(navigateSpy).toHaveBeenCalledWith({ to: '/admin' })
    // No error surfaced on success.
    expect(screen.queryByTestId('error-message')).toBeNull()
  })

  it.each([
    {
      status: 401,
      label: '401 Unauthorized',
      body: { message: 'Unauthorized' },
    },
    {
      status: 500,
      label: '500 Server Error',
      body: { message: 'boom' },
    },
  ])(
    'shows error-message and does NOT store key or navigate on non-204 ($label)',
    async ({ status, body }) => {
      server.use(
        http.get(VERIFY_URL, () => HttpResponse.json(body, { status })),
      )
      const user = userEvent.setup()
      render(<LoginComponent />)

      await user.type(screen.getByTestId('api-key-input'), CANDIDATE_KEY)
      await user.click(screen.getByTestId('submit-button'))

      // error-message appears; key never persisted; navigation never called.
      expect(await screen.findByTestId('error-message')).toBeVisible()
      expect(localStorage.getItem(KEY_STORAGE)).toBeNull()
      expect(navigateSpy).not.toHaveBeenCalled()
    },
  )

  it('shows error-message and does NOT store key or navigate on a network error', async () => {
    // HttpResponse.error() makes fetch reject — the component's catch branch
    // maps this to the same "Key 无效" message.
    server.use(http.get(VERIFY_URL, () => HttpResponse.error()))
    const user = userEvent.setup()
    render(<LoginComponent />)

    await user.type(screen.getByTestId('api-key-input'), CANDIDATE_KEY)
    await user.click(screen.getByTestId('submit-button'))

    expect(await screen.findByTestId('error-message')).toBeVisible()
    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()
    expect(navigateSpy).not.toHaveBeenCalled()
  })

  it('disables the submit button while probing and re-enables after failure', async () => {
    // Delay the handler response so we can observe the in-flight loading state.
    // The setTimeout lives inside the MSW handler (simulating network latency),
    // NOT used as a test wait — we rely on findBy/waitFor for resolution.
    let resolveResponse!: () => void
    const pending = new Promise<void>((resolve) => {
      resolveResponse = resolve
    })
    server.use(
      http.get(VERIFY_URL, async () => {
        await pending
        return HttpResponse.json({ message: 'Unauthorized' }, { status: 401 })
      }),
    )
    const user = userEvent.setup()
    render(<LoginComponent />)

    await user.type(screen.getByTestId('api-key-input'), CANDIDATE_KEY)
    // Use a non-awaiting click dispatch path: userEvent.click awaits, but the
    // handler is pending, so we instead kick submit off and assert before
    // resolution by querying synchronously after the submit handler runs.
    const submitButton = screen.getByTestId('submit-button')
    // Trigger submit without awaiting the pending response.
    void user.click(submitButton)

    await waitFor(() => {
      expect(submitButton).toBeDisabled()
    })

    // Resolve the delayed response → component finishes → re-enabled.
    resolveResponse()
    await waitFor(() => {
      expect(submitButton).not.toBeDisabled()
    })
  })
})

describe('login route — beforeLoad guard', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('redirects to /admin when already authenticated', () => {
    localStorage.setItem(KEY_STORAGE, 'any-stored-key')

    // beforeLoad throws a redirect when isAuthenticated(). The thrown value is
    // NOT an Error — it is a Response-shaped redirect object, so we can't match
    // on `.name`. We assert (a) something was thrown, then (b) the thrown value
    // is a real TanStack redirect targeting /admin, via isRedirect().
    // ctx is unused by the implementation (only reads isAuthenticated()).
    let thrown: unknown
    expect(() => {
      try {
        beforeLoad!(undefined)
      } catch (e) {
        thrown = e
        throw e
      }
    }).toThrow()

    expect(isRedirect(thrown!)).toBe(true)
    expect((thrown as { options: { to: string } }).options.to).toBe('/admin')
  })

  it('does not redirect when not authenticated', () => {
    expect(localStorage.getItem(KEY_STORAGE)).toBeNull()
    // No throw — guard allows the login page to render.
    expect(() => beforeLoad!(undefined)).not.toThrow()
  })
})
