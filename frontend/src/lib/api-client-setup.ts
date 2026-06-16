/**
 * hey-api client 全局鉴权初始化（仅副作用模块）
 *
 * 职责（集中在生产请求路径上，见 FE-D01）：
 *  1. 全局 Bearer 注入：通过 client.setConfig({ auth }) 提供取 token 的回调，
 *     每个 SDK 函数声明的 bearer security 会经 setAuthParams → getAuthToken
 *     自动加 'Bearer ' 前缀。因此任何调用方都不应再手工设置 Authorization。
 *  2. 全局 401 处理：清 Key + 跳转 /auth/login。
 *     - error 拦截器签名: (error, response, request, options)。
 *       error 为解析后的 body（JSON/text）；response 为带 status 的 fetch Response。
 *       网络异常时 response 为 undefined（见 client.gen.ts 的 catch 分支），
 *       此时不清理 Key（避免把网络故障误判为 401）。
 *     - 已在 /auth/login 页时不再跳转，避免循环。
 *
 * 在 app 入口（main.tsx）以副作用 import 或显式调用 installApiClientAuth() 触发一次。
 * 内部用 installed 守卫保证幂等。
 */
import { client } from '@/lib/api-generated/client.gen'
import { clearApiKey, getApiKey } from '@/lib/auth'

let installed = false

export function installApiClientAuth(): void {
  if (installed) return
  installed = true

  client.setConfig({
    auth: () => getApiKey() ?? undefined,
  })

  client.interceptors.error.use((error, response) => {
    if (response?.status === 401) {
      clearApiKey()
      if (
        typeof window !== 'undefined' &&
        !window.location.pathname.startsWith('/auth/login')
      ) {
        window.location.href = '/auth/login'
      }
    }
    return error
  })
}

// 以副作用 import 本模块时自动安装。
installApiClientAuth()
