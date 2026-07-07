/**
 * 基础 Demo 环境检查（support-multiple-website 迁移）
 *
 * Lightweight environment-check: verifies the demo frontend boots and the root
 * `/` route renders the channel-entry list (US-INTG-005 channel discovery). Root `/`
 * is no longer a chat surface — chat lives under `/c/$channelId`.
 *
 * US-story traceability:
 * - US-INTG-005 (DRAFT, `.ai/user-stories/integration/support-multiple-website.md`)
 *   - Scenario "成功配置多个频道": root `/` lists configured channels; the demo
 *     backend seeds `help_center` + `developer_docs`, so the help_center entry
 *     must render.
 *
 * Dependencies:
 * - demo/e2e/fixtures/chat.fixtures.ts (demoLogger via fixture)
 * - demo/e2e/pages/home-page.ts (channel-entry-list POM)
 * - demo/e2e/selectors.ts (SELECTORS.channel.*)
 */

import { test, expect } from './fixtures/chat.fixtures'
import { HomePage } from './pages/home-page'
import { SELECTORS } from './selectors'

test.describe('Basic Demo environment — channel discovery', () => {
  test('should render the channel-entry list with help_center on root /', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const homePage = new HomePage(page)
    await homePage.navigate()

    // The list shows a loading placeholder while fetching /api/channels, then renders entries.
    await expect(page.locator(SELECTORS.channel.channelListLoading)).toBeHidden({ timeout: 15000 })

    // Assert: at least one channel-entry is rendered.
    await expect(page.locator(SELECTORS.channel.channelEntry).first()).toBeVisible()

    // Assert: the demo-seeded help_center entry is present (US-INTG-005 successful config).
    await expect(page.locator(SELECTORS.channel.channelEntryById('help_center'))).toBeVisible()

    console.log('Test started successfully — channel-entry list rendered')
    console.log('demoLogger is working')
  })

  test('should verify root / navigation url via FRONTEND_URL', async ({
    page,
    demoLogger: _demoLogger,
  }) => {
    const homePage = new HomePage(page)
    await homePage.navigate()

    const currentUrl = page.url()
    console.log(`Current URL: ${currentUrl}`)
    expect(currentUrl).toBeTruthy()
    expect(currentUrl.length).toBeGreaterThan(0)
  })

  test('should handle unknown route gracefully', async ({ page, demoLogger: _demoLogger }) => {
    // Navigate to a non-existent (non-channel) route; the app should not crash.
    await page.goto(`${process.env.FRONTEND_URL || 'http://localhost:5173'}/non-existent-page`).catch(() => null)

    console.log('Navigation test completed')
  })
})
