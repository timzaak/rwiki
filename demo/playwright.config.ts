/**
 * Playwright 测试配置
 *
 * 测试策略：以 Demo E2E 为主，验证完整用户流程
 *
 * 配置说明：
 * - timeout: 单个测试最长执行时间（120s）
 * - expect.timeout: 断言等待时间（10s）
 * - workers: 单线程执行，确保测试稳定
 * - screenshot/video/trace: 默认关闭，按需开启
 *
 * 环境变量：
 * - BASE_URL: 后端地址（默认 http://localhost:8080）
 * - UNIFIED_LOG_LEVEL: mini | normal | verbose | silent（默认 mini）
 */

import { defineConfig, devices } from '@playwright/test'

const BASE_URL = process.env.BASE_URL || 'http://localhost:8080'

export default defineConfig({
  testDir: './e2e',
  testMatch: '**/*.e2e.ts',

  timeout: 120 * 1000,
  expect: {
    timeout: 10 * 1000,
  },

  retries: 0,
  fullyParallel: false,
  workers: 1,

  outputDir: 'test-results/artifacts',

  use: {
    baseURL: BASE_URL,
    screenshot: 'off',
    video: 'off',
    trace: 'off',
    actionTimeout: 0,
    navigationTimeout: 15 * 1000,
  },

  projects: [
    {
      name: 'demo-fast',
      use: {
        ...devices['Desktop Chrome'],
        headless: true,
        launchOptions: {
          args: [
            '--lang=en-US',
            '--enable-logging',
            '--log-level=0',
            '--disable-features=TranslateUI',
            '--no-first-run',
            '--no-default-browser-check',
          ],
        },
      },
    },
  ],

  reporter: [
    ['html', { open: 'never' }],
    ['list'],
  ],

  quiet: false,
})
