/**
 * 管理后台全局频道上下文。
 *
 * 单一来源：在 admin 布局顶部由 `AdminChannelProvider` 拉取 `listChannels()` 并维护
 * 当前 `channelId`，所有 admin 页（文档列表/上传/批处理发布取消删除/低召回）通过
 * `useAdminChannel()` 消费，避免 prop drilling，并保证切换频道后各列表按新 channelId 重取。
 *
 * `channelId: string | null`：null 表示尚未选定或无可用频道，消费方据此跳过请求与禁用操作。
 *
 * 状态：
 *   - 加载中：`loading=true`，渲染 selector 旁「Loading…」。
 *   - 成功且非空：默认选首项（或命中 localStorage 且在列表内则用之）。
 *   - 成功但空数组：`channelId=null`（系统应至少一频道，空态为异常，提示联系管理员）。
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
import { listChannels } from '@/lib/api-generated/sdk.gen'
import type { ChannelItem } from '@/lib/api-generated/types.gen'

const STORAGE_KEY = 'rwiki_admin_channel_id'
const MAX_ATTEMPTS = 3
const RETRY_DELAY_MS = 5000

export interface AdminChannelValue {
  channelId: string | null
  setChannelId: (id: string) => void
  channels: ChannelItem[]
  loading: boolean
  error: string | null
  retry: () => void
}

const AdminChannelContext = createContext<AdminChannelValue | null>(null)

function readStoredChannelId(): string | null {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY)
    return value && value.trim() !== '' ? value : null
  } catch {
    // localStorage 不可用时静默降级（隐私模式/SSR）。
    return null
  }
}

function writeStoredChannelId(id: string): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, id)
  } catch {
    // 写入失败不阻断功能。
  }
}

export function AdminChannelProvider({ children }: { children: ReactNode }) {
  const [channels, setChannels] = useState<ChannelItem[]>([])
  const [channelId, setChannelIdState] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  // 自动重试计数：跨闭包用 ref，避免重试定时器读到过期 state。
  const attemptRef = useRef(0)
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await listChannels()
      if (result.error || (result.response && !result.response.ok)) {
        throw new Error('Failed to load channels')
      }
      const fetched = result.data?.channels ?? []
      setChannels(fetched)
      if (fetched.length === 0) {
        // 空频道为异常态：保持 channelId=null，消费方禁用操作并提示联系管理员。
        setChannelIdState(null)
        attemptRef.current = 0
        setLoading(false)
        return
      }
      // 成功：优先用 localStorage 命中（且需在列表内），否则默认首项。
      const stored = readStoredChannelId()
      const initial =
        stored && fetched.some((c) => c.id === stored)
          ? stored
          : fetched[0].id
      setChannelIdState(initial)
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
        setError('Failed to load channels')
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

  const setChannelId = useCallback((id: string) => {
    setChannelIdState(id)
    writeStoredChannelId(id)
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
    <AdminChannelContext.Provider
      value={{ channelId, setChannelId, channels, loading, error, retry }}
    >
      {children}
    </AdminChannelContext.Provider>
  )
}

/**
 * 读取管理后台当前频道上下文。必须在 `AdminChannelProvider` 内调用，否则抛错——
 * 这强制 admin 路由在渲染布局前先包裹 Provider。
 */
export function useAdminChannel(): AdminChannelValue {
  const value = useContext(AdminChannelContext)
  if (value === null) {
    throw new Error('useAdminChannel must be used within an AdminChannelProvider')
  }
  return value
}
