/**
 * Demo test fixtures (simplified)
 *
 * Re-exports a demoLogger fixture compatible with chat.fixtures.
 * Kept for backward compatibility with any test that imports from this path.
 *
 * Usage:
 * ```typescript
 * import { test, expect } from '../fixtures/demo-auth.fixtures'
 *
 * test('my test', async ({ page, demoLogger }) => {
 *   // ...
 * })
 * ```
 */

import { test as base } from '@playwright/test'
import { UnifiedLogger } from 'playwright-unified-logger'

export const test = base.extend<{
  demoLogger: UnifiedLogger
}>({
  demoLogger: async ({ page }, use, testInfo) => {
    const logger = new UnifiedLogger(page, testInfo.title)
    await use(logger)
    logger.printSummary('[Demo] Test Summary')
    await logger.finalize()
  },
})

export { expect } from '@playwright/test'
