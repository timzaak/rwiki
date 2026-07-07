import { describe, it, expect } from 'vitest'
import { isRedirect } from '@tanstack/router-core'

import '@/test/mocks/server'

/**
 * 主页 `/` —— beforeLoad 重定向至默认频道（FE-T02）。
 *
 * 镜像 routes/admin/__tests__/admin.test.tsx 的 beforeLoad 守卫断言方式：
 * 确保用户进入 demo 即落到有数据的频道，避免误入空态 503。
 */
describe('landing — redirect to default channel', () => {
  it('beforeLoad redirects to /c/help_center', async () => {
    const { Route: LandingRoute } = await import('@/routes/index')
    const beforeLoad = LandingRoute.options.beforeLoad as
      | ((ctx: unknown) => unknown)
      | undefined

    expect(typeof beforeLoad).toBe('function')

    let thrown: unknown
    try {
      beforeLoad!(undefined)
    } catch (e) {
      thrown = e
    }

    expect(isRedirect(thrown!)).toBe(true)
    expect((thrown as { options: { to: string } }).options.to).toBe(
      '/c/$channelId',
    )
    expect(
      (thrown as { options: { params: { channelId: string } } }).options.params
        .channelId,
    ).toBe('help_center')
  })
})
