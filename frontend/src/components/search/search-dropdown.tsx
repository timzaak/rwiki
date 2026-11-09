import { Fragment, memo, useMemo } from 'react'
import {
  AlertCircleIcon,
  ExternalLinkIcon,
  LoaderCircleIcon,
  SparklesIcon,
} from 'lucide-react'

import type { SearchResultItem } from '@/lib/api-generated/types.gen'
import type { SearchState } from '@/hooks/use-search'
import { getHighlightTerms, highlightSegments } from '@/lib/highlight'

/**
 * 搜索下拉（纯展示，不发请求、不感知开合规则）。
 *
 * 加载与无结果用 role="status"、错误用 role="alert"；结果按接口返回顺序
 * 渲染、不按 score 重排（page 级排序下相邻分数不单调）；link 为空/缺失的
 * 条目渲染为非交互块，不跳转。标题与摘要按产生本批结果的检索词高亮
 *（US-CORE-043，切词口径见 lib/highlight.ts）。
 *
 * 「用 AI 回答」入口（US-CORE-044）置顶于结果列表之上、不随列表滚动，只在
 * 有结果时出现：零命中时聊天同样检索不到依据，只会多花一次 LLM 调用。
 *
 * `data-search-option` 标记方向键可达的选项（AI 入口与带链接条目，按 DOM
 * 顺序），焦点移动由 SearchBox 负责；无链接条目非交互，不在其中。
 */
export function SearchDropdown({
  state,
  onAskAi,
}: {
  state: SearchState
  onAskAi?: (query: string) => void
}) {
  // 高亮词表只随结果批次变化（state 引用在两次检索之间不变）：结果展开时
  // 继续改草稿会逐键重渲本组件，不能逐键重跑切词；memo(ResultBody) 再让
  // 每条结果的全文匹配只在 item/terms 变化时重算。
  const terms = useMemo(
    () => (state.status === 'success' ? getHighlightTerms(state.query) : []),
    [state],
  )

  if (state.status === 'idle') return null

  return (
    <div
      data-testid="search-dropdown"
      className="absolute left-0 top-full z-20 mt-2 w-full rounded-lg border border-border/60 bg-popover p-2 text-popover-foreground shadow-lg"
    >
      {state.status === 'loading' ? (
        <p
          data-testid="search-loading"
          role="status"
          className="flex items-center gap-2 px-2 py-2.5 text-sm text-muted-foreground"
        >
          <LoaderCircleIcon className="size-4 animate-spin" />
          正在搜索…
        </p>
      ) : state.status === 'error' ? (
        <p
          data-testid="search-error"
          role="alert"
          className="flex items-center gap-2 px-2 py-2.5 text-sm text-destructive"
        >
          <AlertCircleIcon className="size-4 shrink-0" />
          {state.message}
        </p>
      ) : state.results.length === 0 ? (
        <p
          data-testid="search-empty"
          role="status"
          className="px-2 py-2.5 text-sm text-muted-foreground"
        >
          没有找到相关内容，换个关键词试试
        </p>
      ) : (
        <>
          {onAskAi ? (
            <div className="mb-1 border-b border-border/60 pb-1">
              <button
                type="button"
                data-testid="search-ask-ai"
                data-search-option
                onClick={() => onAskAi(state.query)}
                className="flex w-full items-center gap-2 rounded-md p-2 text-left text-sm outline-none transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-primary/15"
              >
                <SparklesIcon className="size-4 shrink-0 text-primary" />
                <span className="truncate">用 AI 回答「{state.query}」</span>
              </button>
            </div>
          ) : null}
          <ul
            data-testid="search-result-list"
            className="flex max-h-[50vh] flex-col gap-1 overflow-y-auto"
          >
            {state.results.map((item, index) => (
              <li key={index} data-testid="search-result-item">
                {item.link ? (
                  <a
                    data-testid="search-result-link"
                    data-search-option
                    href={item.link}
                    target="_blank"
                    rel="noopener noreferrer"
                    className="block rounded-md p-2 outline-none transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-primary/15"
                  >
                    <ResultBody item={item} terms={terms} />
                  </a>
                ) : (
                  <div className="block rounded-md p-2">
                    <ResultBody item={item} terms={terms} />
                  </div>
                )}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}

const ResultBody = memo(function ResultBody({
  item,
  terms,
}: {
  item: SearchResultItem
  terms: string[]
}) {
  return (
    <>
      <p className="truncate text-sm font-medium">
        <Highlighted text={item.title} terms={terms} />
      </p>
      <p className="mt-0.5 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
        <Highlighted text={item.content} terms={terms} />
      </p>
      {item.link ? (
        <span className="mt-1 flex items-center gap-1 truncate text-xs text-primary">
          <span className="truncate">{item.link}</span>
          <ExternalLinkIcon className="size-3 shrink-0" />
        </span>
      ) : (
        <span className="mt-1 block truncate text-xs text-muted-foreground/70">
          暂无来源链接
        </span>
      )}
    </>
  )
})

function Highlighted({ text, terms }: { text: string; terms: string[] }) {
  return highlightSegments(text, terms).map((segment, index) =>
    segment.match ? (
      <mark
        key={index}
        data-testid="search-highlight"
        className="rounded-[2px] bg-primary/15 px-0.5 text-inherit"
      >
        {segment.text}
      </mark>
    ) : (
      <Fragment key={index}>{segment.text}</Fragment>
    ),
  )
}
