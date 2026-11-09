/**
 * 主站频道搜索 hook（SearchBox 专用）
 *
 * 仓库未安装 @tanstack/react-query，数据访问沿用既有约定
 * （自定义 hook + 生成客户端 + 局部 state + cancelled 守卫，
 * 同 use-low-recall-records.ts）。
 *
 * channelId 由频道路由 prop 直传（同 ChannelChatModalMount 先例：直子组件
 * 不经 channel-id-context，该 context 服务聊天深层组件树）。
 */
import { useEffect, useState } from 'react'
import { searchChunks } from '@/lib/api-generated/sdk.gen'
import type { ErrorResponse, SearchResultItem } from '@/lib/api-generated/types.gen'

const SEARCH_FAILED_MESSAGE = '搜索失败，请稍后重试'

export type SearchState =
  | { status: 'idle' }
  | { status: 'loading' }
  // query = 产生这批结果的检索词（不是之后改过的输入框草稿）：高亮与
  // 「用 AI 回答」都跟随它，保证与所展示的结果同源。
  | { status: 'success'; query: string; results: SearchResultItem[] }
  | { status: 'error'; message: string }

interface SearchSubmission {
  query: string
}

export function useSearch(channelId: string) {
  const [submission, setSubmission] = useState<SearchSubmission | null>(null)
  const [state, setState] = useState<SearchState>({ status: 'idle' })

  useEffect(() => {
    if (submission === null) return
    let cancelled = false
    searchChunks({ body: { query: submission.query, channelId } })
      .then((result) => {
        if (cancelled) return
        if (result.error) {
          // 仅 400 的业务提示（如「频道 xxx 未配置」）原文呈现；500 与网络
          // 失败一律固定文案，不外露内部错误细节。
          // SDK 把 error 静态类型化为 unknown（真值收窄后是 {}），按
          // OpenAPI 契约（400 → ErrorResponse）收窄，运行时再校验 message。
          const err = result.error as ErrorResponse | undefined
          const raw =
            result.response?.status === 400 &&
            typeof err?.message === 'string'
              ? err.message
              : ''
          setState({
            status: 'error',
            message: raw || SEARCH_FAILED_MESSAGE,
          })
        } else if (result.data) {
          setState({
            status: 'success',
            query: submission.query,
            results: result.data.results,
          })
        }
      })
      .catch(() => {
        if (cancelled) return
        setState({ status: 'error', message: SEARCH_FAILED_MESSAGE })
      })
    return () => {
      cancelled = true
    }
  }, [submission, channelId])

  function search(query: string) {
    // 相同关键词在途时忽略重复提交，避免连按回车成倍放大检索成本。
    if (submission?.query === query && state.status === 'loading') return
    // 同步置 loading：提交开下拉的同一次 commit 不携带旧结果/旧错误一帧
    //（setState 放 effect 里要等 paint 后，晚一拍）。
    setSubmission({ query })
    setState({ status: 'loading' })
  }

  function reset() {
    setSubmission(null)
    setState({ status: 'idle' })
  }

  return { state, search, reset }
}
