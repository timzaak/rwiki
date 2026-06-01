/**
 * Chat-specific test fixtures
 *
 * Extends Playwright base test (no auth) with:
 * - demoLogger: UnifiedLogger auto-finalized after each test
 *
 * This app has no authentication, so we do NOT use demo-auth.fixtures.
 *
 * Usage:
 * ```typescript
 * import { test, expect } from '../fixtures/chat.fixtures'
 *
 * test('my chat test', async ({ page, demoLogger }) => {
 *   // ...
 * })
 * ```
 */

import { test as base } from '@playwright/test'
import { UnifiedLogger } from 'playwright-unified-logger'

/**
 * Frontend dev server URL (Vite, port 5173).
 *
 * IMPORTANT: BasePage.goto() prefixes with BASE_URL (port 8080, backend-only).
 * Page objects MUST use FRONTEND_URL for navigation instead.
 */
export const FRONTEND_URL = process.env.FRONTEND_URL || 'http://localhost:5173'

export const test = base.extend<{
  demoLogger: UnifiedLogger
}>({
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    logger.printSummary('[Chat Demo] Test Summary')
    await logger.finalize()
  },
})

export { expect } from '@playwright/test'
