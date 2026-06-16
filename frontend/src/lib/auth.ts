/**
 * API Key 鉴权存储
 *
 * 仅负责 localStorage 中 rwiki API Key 的存取/清理/判定。
 * 不含 Bearer 注入逻辑（集中在 src/lib/api-client-setup.ts），
 * 也不含登录 UI（见 FE-D02）。
 */
const KEY = 'rwiki_api_key'

export const getApiKey = (): string | null => localStorage.getItem(KEY)

export const setApiKey = (key: string): void => {
  localStorage.setItem(KEY, key)
}

export const clearApiKey = (): void => {
  localStorage.removeItem(KEY)
}

export const isAuthenticated = (): boolean => getApiKey() !== null
