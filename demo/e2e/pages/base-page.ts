/**
 * Base Page Object
 *
 * 所有 Page Object 的基类，封装通用功能：
 * - 导航（goto）
 * - 等待元素可见/隐藏
 * - 智能点击（先等待可见）
 * - 表单填写（含 blur 触发验证）
 * - 截图（调试用）
 *
 * 使用方式：
 * ```typescript
 * class MyPage extends BasePage {
 *   constructor(page: Page, logger?: UnifiedLogger) {
 *     super(page, logger)
 *   }
 * }
 * ```
 */

import { Page, Locator, expect } from '@playwright/test'
import type { UnifiedLogger } from 'playwright-unified-logger'

const BASE_URL = process.env.BASE_URL || 'http://localhost:8080'

export class BasePage {
  protected logger?: UnifiedLogger

  constructor(public readonly page: Page, logger?: UnifiedLogger) {
    this.logger = logger
  }

  /**
   * 导航到指定路径
   */
  async goto(path: string, waitForSelector?: string): Promise<void> {
    const url = path.startsWith('http') ? path : `${BASE_URL}${path}`
    await this.page.goto(url)

    if (waitForSelector) {
      await expect(this.page.locator(waitForSelector)).toBeVisible()
    }
  }

  protected async waitForLoad(state: 'load' | 'domcontentloaded' | 'networkidle' = 'domcontentloaded'): Promise<void> {
    await this.page.waitForLoadState(state)
  }

  protected async waitForVisible(locator: Locator, timeout: number = 5000): Promise<void> {
    await expect(locator).toBeVisible({ timeout })
  }

  protected async waitForHidden(locator: Locator, timeout: number = 5000): Promise<void> {
    await expect(locator).toBeHidden({ timeout })
  }

  /**
   * 智能点击 — 先等待元素可见再点击
   */
  public async smartClick(element: Locator, force: boolean = false): Promise<void> {
    await expect(element).toBeVisible()
    await element.click({ force })
  }

  getUrl(): string {
    return this.page.url()
  }

  async isVisible(locator: Locator): Promise<boolean> {
    return await locator.isVisible().catch(() => false)
  }

  /**
   * 填写表单字段 — 自动全选替换 + blur 触发验证
   */
  protected async fillField(locator: Locator, value: string): Promise<void> {
    await expect(locator).toBeVisible()
    await locator.selectText()
    await locator.fill(value)
    await locator.blur()

    // 验证值已提交
    await expect(async () => {
      const inputValue = await locator.inputValue()
      expect(inputValue).toBe(value)
    }).toPass({ timeout: 2000 })
  }

  protected async getText(locator: Locator): Promise<string> {
    await expect(locator).toBeVisible()
    return await locator.textContent() || ''
  }

  protected async screenshot(name: string): Promise<void> {
    await this.page.screenshot({ path: `test-results/screenshots/${name}.png` })
  }
}
