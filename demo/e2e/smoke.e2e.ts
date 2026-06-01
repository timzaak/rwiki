/**
 * 冒烟测试 — 不依赖后端服务
 *
 * 验证 Playwright 测试基础设施是否正常工作。
 * 这是项目初始化后应运行的第一个测试。
 * 不需要后端、数据库或 Redis 运行。
 *
 * 运行方式：
 *   cd demo && npx playwright test e2e/smoke.e2e.ts
 *
 * 如果此测试失败，说明测试环境本身有问题（依赖安装、Playwright 等）。
 */

import { test, expect } from '@playwright/test'

test.describe('Smoke Test', () => {
  test('playwright can launch browser', async ({ page }) => {
    await page.goto('about:blank')
    const title = await page.title()
    expect(title).toBeDefined()
  })

  test('page navigation works', async ({ page }) => {
    await page.goto('about:blank')
    await page.setContent('<html><body><h1 id="test">Hello</h1></body></html>')

    const heading = page.locator('#test')
    await expect(heading).toBeVisible()
    await expect(heading).toHaveText('Hello')
  })

  test('basic assertions work', async () => {
    expect(true).toBe(true)
    expect([1, 2, 3]).toHaveLength(3)
    expect('hello world').toContain('world')
  })

  test('can import test helpers', async ({ page: _page }) => {
    // 验证模块导入路径正确
    const { SELECTORS } = await import('./selectors')
    expect(SELECTORS).toBeDefined()
    expect(SELECTORS.chat).toBeDefined()
    expect(SELECTORS.common).toBeDefined()
  })
})
