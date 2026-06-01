/**
 * 环境验证工具
 *
 * 在测试运行前验证后端服务和数据库是否就绪。
 *
 * 使用方式：
 * ```typescript
 * import { verifyTestEnvironment } from './helpers/environment-setup'
 * await verifyTestEnvironment(page)
 * ```
 *
 * 依赖：
 * - 后端提供 GET /health 接口（返回 { status: "healthy", database: "connected" }）
 */

import { Page } from '@playwright/test'

export const BASE_URL = process.env.BASE_URL || 'http://localhost:8080'

export interface VerifyEnvironmentOptions {
  /** 跳过数据库检查 */
  skipDatabaseCheck?: boolean
}

interface ValidationResult {
  healthy: boolean
  response?: {
    status: string
    database: string
  }
  errors?: string[]
}

/**
 * 验证测试环境状态
 */
export async function verifyTestEnvironment(
  page: Page,
  options: VerifyEnvironmentOptions = {}
): Promise<void> {
  console.log('[Env] 验证测试环境...')

  await verifyBackendConnections({ skipDatabaseCheck: options.skipDatabaseCheck ?? false })

  console.log('[Env] 环境验证通过')
}

async function verifyBackendConnections(options: {
  skipDatabaseCheck: boolean
}): Promise<void> {
  const result = await validateBackendHealth({
    maxRetries: 3,
    retryDelay: 2000,
    timeout: 10000,
  })

  if (!result.healthy) {
    throw new Error(`Backend health check failed:\n${result.errors?.join('\n') || 'Unknown error'}`)
  }

  if (!options.skipDatabaseCheck && result.response?.database !== 'connected') {
    throw new Error(`数据库连接失败: ${result.response?.database ?? 'unknown'}`)
  }

  console.log('[Env] 数据库连接正常')
}

async function validateBackendHealth(options: {
  maxRetries: number
  retryDelay: number
  timeout: number
}): Promise<ValidationResult> {
  const { maxRetries, retryDelay, timeout } = options

  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      const response = await fetch(`${BASE_URL}/health`, {
        method: 'GET',
        signal: AbortSignal.timeout(timeout),
      })

      if (response.ok) {
        const data = await response.json().catch(() => ({}))
        return {
          healthy: true,
          response: {
            status: data.status ?? 'unknown',
            database: data.database ?? 'unknown',
          },
        }
      }
    } catch (error) {
      if (attempt < maxRetries - 1) {
        console.log(`[Env] 健康检查失败，重试 ${attempt + 1}/${maxRetries}...`)
        await new Promise(resolve => setTimeout(resolve, retryDelay))
      } else {
        return {
          healthy: false,
          errors: [`Health check failed after ${maxRetries} attempts: ${error}`],
        }
      }
    }
  }

  return { healthy: false, errors: ['Health check failed: Max retries exceeded'] }
}
