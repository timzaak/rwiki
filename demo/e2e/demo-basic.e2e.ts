/**
 * 基础 Demo 测试
 *
 * 验证测试环境和 UnifiedLogger 是否正常工作。
 * 这是创建项目后应运行的第一个测试。
 *
 * 运行方式：
 *   cd demo && npm test
 *   cd demo && npm run test:headed  # 有头模式
 *
 * 环境变量：
 *   BASE_URL=http://localhost:8080 npm test
 *   UNIFIED_LOG_LEVEL=verbose npm test  # 详细日志
 */

import { test, expect } from './fixtures/chat.fixtures'

test.describe('Basic Demo Test', () => {
  test('should capture logs', async ({ page, demoLogger: _demoLogger }) => {
    // 导航到首页
    await page.goto('/')

    // 测试代码中的 console.log 会被 TestCodeLogger 捕获
    console.log('Test started successfully')
    console.log('demoLogger is working')

    // demoLogger 自动捕获所有日志并在测试结束时打印摘要
  })

  test('should verify page navigation', async ({ page, demoLogger: _demoLogger }) => {
    await page.goto('/')

    const currentUrl = page.url()
    console.log(`Current URL: ${currentUrl}`)

    expect(currentUrl).toBeTruthy()
    expect(currentUrl.length).toBeGreaterThan(0)
  })

  test('should handle page errors gracefully', async ({ page, demoLogger: _demoLogger }) => {
    // 尝试导航到不存在的页面
    await page.goto('/non-existent-page').catch(() => null)

    // 测试应正常完成，demoLogger 会捕获任何控制台错误
    console.log('Navigation test completed')
  })
})
