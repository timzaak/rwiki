import { AlertCircleIcon, ExternalLinkIcon, LoaderCircleIcon } from 'lucide-react'

import type { SearchResultItem } from '@/lib/api-generated/types.gen'
import type { SearchState } from '@/hooks/use-search'

/**
 * 搜索下拉（纯展示，不发请求、不感知开合规则）。
 *
 * 加载与无结果用 role="status"、错误用 role="alert"；结果按接口返回顺序
 * 渲染、不按 score 重排（page 级排序下相邻分数不单调）；link 为空/缺失的
 * 条目渲染为非交互块，不跳转。
 */
export function SearchDropdown({ state }: { state: SearchState }) {
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
        <ul
          data-testid="search-result-list"
          className="flex max-h-[50vh] flex-col gap-1 overflow-y-auto"
        >
          {state.results.map((item, index) => (
            <li key={index} data-testid="search-result-item">
              {item.link ? (
                <a
                  data-testid="search-result-link"
                  href={item.link}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="block rounded-md p-2 outline-none transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-primary/15"
                >
                  <ResultBody item={item} />
                </a>
              ) : (
                <div className="block rounded-md p-2">
                  <ResultBody item={item} />
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}

function ResultBody({ item }: { item: SearchResultItem }) {
  return (
    <>
      <p className="truncate text-sm font-medium">{item.title}</p>
      <p className="mt-0.5 line-clamp-2 text-xs leading-relaxed text-muted-foreground">
        {item.content}
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
}
