/**
 * 管理后台全局站点上下文。
 *
 * 单一来源：在 admin 布局顶部由 `AdminSiteProvider` 拉取 `listSites()` 并维护
 * 当前 `siteId`，所有 admin 页（文档列表/上传/批处理发布取消删除/低召回）通过
 * `useAdminSite()` 消费，避免 prop drilling，并保证切换站点后各列表按新 siteId 重取。
 *
 * `siteId: string | null`：null 表示尚未选定或无可用站点，消费方据此跳过请求与禁用操作。
 *
 * 状态：
 *   - 加载中：`loading=true`，渲染 selector 旁「Loading…」。
 *   - 成功且非空：默认选首项（或命中 localStorage 且在列表内则用之）。
 *   - 成功但空数组：`siteId=null`（系统应至少一站点，空态为异常，提示联系管理员）。
 *   - 失败：`error` 态 + `retry`，按设计 §4.4.2 自动重试最多 3 次、间隔 5s。
 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { listSites } from '@/lib/api-generated/sdk.gen'
import type { SiteItem } from '@/lib/api-generated/types.gen'

const STORAGE_KEY = 'rwiki_admin_site_id'
const MAX_ATTEMPTS = 3
const RETRY_DELAY_MS = 5000

export interface AdminSiteValue {
  siteId: string | null
  setSiteId: (id: string) => void
  sites: SiteItem[]
  loading: boolean
  error: string | null
  retry: () => void
}

const AdminSiteContext = createContext<AdminSiteValue | null>(null)

function readStoredSiteId(): string | null {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY)
    return value && value.trim() !== '' ? value : null
  } catch {
    // localStorage 不可用时静默降级（隐私模式/SSR）。
    return null
  }
}

function writeStoredSiteId(id: string): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, id)
  } catch {
    // 写入失败不阻断功能。
  }
}

export function AdminSiteProvider({ children }: { children: ReactNode }) {
  const [sites, setSites] = useState<SiteItem[]>([])
  const [siteId, setSiteIdState] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // 自动重试计数：跨闭包用 ref，避免重试定时器读到过期 state。
  const attemptRef = useRef(0)
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await listSites()
      if (result.error || (result.response && !result.response.ok)) {
        throw new Error('Failed to load sites')
      }
      const fetched = result.data?.sites ?? []
      setSites(fetched)
      if (fetched.length === 0) {
        // 空站点为异常态：保持 siteId=null，消费方禁用操作并提示联系管理员。
        setSiteIdState(null)
        attemptRef.current = 0
        setLoading(false)
        return
      }
      // 成功：优先用 localStorage 命中（且需在列表内），否则默认首项。
      const stored = readStoredSiteId()
      const initial =
        stored && fetched.some((s) => s.id === stored)
          ? stored
          : fetched[0].id
      setSiteIdState(initial)
      attemptRef.current = 0
      setLoading(false)
    } catch {
      attemptRef.current += 1
      if (attemptRef.current < MAX_ATTEMPTS) {
        // 设计 §4.4.2：5s 间隔自动重试最多 3 次（首次失败即第 1 次 attempt）。
        setLoading(true)
        retryTimerRef.current = setTimeout(() => {
          void load()
        }, RETRY_DELAY_MS)
      } else {
        setError('Failed to load sites')
        setLoading(false)
      }
    }
  }, [])

  // 挂载时拉取一次。
  useEffect(() => {
    void load()
    return () => {
      if (retryTimerRef.current) {
        clearTimeout(retryTimerRef.current)
        retryTimerRef.current = null
      }
      attemptRef.current = 0
    }
  }, [load])

  const setSiteId = useCallback((id: string) => {
    setSiteIdState(id)
    writeStoredSiteId(id)
  }, [])

  // 手动重试：重置计数并立即拉取。
  const retry = useCallback(() => {
    if (retryTimerRef.current) {
      clearTimeout(retryTimerRef.current)
      retryTimerRef.current = null
    }
    attemptRef.current = 0
    void load()
  }, [load])

  return (
    <AdminSiteContext.Provider
      value={{ siteId, setSiteId, sites, loading, error, retry }}
    >
      {children}
    </AdminSiteContext.Provider>
  )
}

/**
 * 读取管理后台当前站点上下文。必须在 `AdminSiteProvider` 内调用，否则抛错——
 * 这强制 admin 路由在渲染布局前先包裹 Provider。
 */
export function useAdminSite(): AdminSiteValue {
  const value = useContext(AdminSiteContext)
  if (value === null) {
    throw new Error('useAdminSite must be used within an AdminSiteProvider')
  }
  return value
}
