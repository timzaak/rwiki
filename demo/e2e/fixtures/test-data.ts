/**
 * 测试数据管理
 *
 * 集中管理 E2E 测试使用的测试数据和辅助函数。
 * 修改此文件以匹配项目实际数据需求。
 */

export interface TestAccount {
  email: string
  password: string
  realmId: string
}

export interface TestRealm {
  id: string
  name: string
  adminEmail: string
}

/**
 * 测试 Realm 数据 — 根据项目修改
 */
export const TEST_REALMS: Record<string, TestRealm> = {
  admin: {
    id: 'admin',
    name: 'Admin Realm',
    adminEmail: 'admin@example.com',
  },
}

/**
 * 测试角色
 */
export const TEST_ROLES = {
  USER: 'user',
  ADMIN: 'admin',
  REALM_ADMIN: 'realm-admin',
} as const

/**
 * 生成随机测试用户数据
 */
export function generateTestUser(options?: {
  email?: string
  password?: string
  nickname?: string
  realmId?: string
}): TestAccount & { nickname: string } {
  const timestamp = Date.now()
  const random = Math.floor(Math.random() * 1000)

  return {
    email: options?.email || `test-user-${timestamp}-${random}@demo.com`,
    password: options?.password || 'password123',
    nickname: options?.nickname || `Test User ${timestamp}`,
    realmId: options?.realmId || 'admin',
  }
}
