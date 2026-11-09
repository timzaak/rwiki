import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { SearchIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useSearch } from '@/hooks/use-search'
import { SearchDropdown } from './search-dropdown'

/**
 * 主站频道搜索框：回车或「搜索」按钮触发检索，输入过程零请求。
 * 仅由频道路由 ready 分支挂载，不经 components/chat 共享（Widget 不加载）。
 * 「用 AI 回答」由挂载方经 onAskAi 注入（打开聊天并发出检索词），本组件
 * 不依赖聊天 context/store；未注入时不渲染该入口。
 */
export function SearchBox({
  channelId,
  onAskAi,
}: {
  channelId: string
  onAskAi?: (query: string) => void
}) {
  const [draft, setDraft] = useState('')
  const [open, setOpen] = useState(false)
  const { state, search, reset } = useSearch(channelId)
  const rootRef = useRef<HTMLDivElement>(null)
  const inputRef = useRef<HTMLInputElement>(null)

  const trimmed = draft.trim()

  useEffect(() => {
    if (!open) return
    // 区域外 pointerdown/focusin 收起。Safari 点击链接不聚焦，根节点 onBlur
    // 会在 click 之前收起面板导致链接点击丢失，故用 document 级监听。
    const isOutside = (target: EventTarget | null) =>
      target instanceof Node &&
      rootRef.current !== null &&
      !rootRef.current.contains(target)
    const onPointerDown = (e: PointerEvent) => {
      if (isOutside(e.target)) setOpen(false)
    }
    const onFocusIn = (e: FocusEvent) => {
      if (isOutside(e.target)) setOpen(false)
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('focusin', onFocusIn)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('focusin', onFocusIn)
    }
  }, [open])

  function submit() {
    if (!trimmed) return
    search(trimmed)
    setOpen(true)
  }

  function askAi(query: string) {
    // 聊天弹窗接手：先收起下拉（同「点击聊天按钮先收起下拉」），关键词保留。
    setOpen(false)
    onAskAi?.(query)
  }

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'Escape') {
      setOpen(false)
      inputRef.current?.focus()
      return
    }
    // 组合输入中的 Enter 是确认候选词、↑/↓ 在挑候选词（Safari 以 keyCode
    // 229 上报组合末尾），此时既不检索也不移动焦点。
    if (e.nativeEvent.isComposing || e.keyCode === 229) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      moveFocus(e)
      return
    }
    if (e.key !== 'Enter' || e.target !== inputRef.current) return
    e.preventDefault()
    submit()
  }

  /**
   * 方向键在输入框与下拉选项（`data-search-option`）间移动真实焦点：输入框
   * 是第 0 站，↓ 在末项夹住、↑ 回到输入框即止（输入框里的 ↑/↓ 在没有可去
   * 站点时留给光标）。选项上的 Enter 由浏览器原生激活（链接开新标签页、AI
   * 按钮触发），滚动跟随与焦点样式也交给浏览器。
   */
  function moveFocus(e: KeyboardEvent<HTMLDivElement>) {
    const options = Array.from(
      rootRef.current?.querySelectorAll<HTMLElement>('[data-search-option]') ?? [],
    )
    const stops = [inputRef.current, ...options]
    const index = stops.indexOf(e.target as HTMLElement)
    if (index === -1) return
    if (e.key === 'ArrowUp') {
      if (index === 0) return
      e.preventDefault()
      stops[index - 1]?.focus()
      return
    }
    // 输入框上收起时 ↓ 重新展开（同点击输入框，不重新检索）。
    if (index === 0 && !open) {
      e.preventDefault()
      setOpen(true)
      return
    }
    if (index < stops.length - 1) {
      e.preventDefault()
      stops[index + 1]?.focus()
    } else if (stops.length > 1) {
      // 末项停住（不绕回）：仍拦截 ↓，避免页面随按键滚动。下拉只有输入框
      // 一个站点（首搜前/加载中/零命中/失败）时不拦截，让 ↓ 保留输入框
      // 原生行为——单行输入框把光标移到末尾。
      e.preventDefault()
    }
  }

  function handleInputChange(value: string) {
    setDraft(value)
    // 清空输入即放弃：收起下拉，reset 置 idle 使在途结果到达后不再出现。
    if (value.trim() === '') {
      reset()
      setOpen(false)
    }
  }

  return (
    <div
      ref={rootRef}
      role="search"
      className="relative"
      onKeyDown={handleKeyDown}
    >
      <div className="flex gap-2">
        <input
          ref={inputRef}
          data-testid="search-input"
          // 不用 type="search"：Chromium/WebKit 中 Esc 会清空其值，
          // 与「Esc 收起、关键词保留」冲突。
          type="text"
          value={draft}
          onChange={(e) => handleInputChange(e.target.value)}
          onClick={() => setOpen(true)}
          placeholder="搜索知识库…"
          aria-label="搜索知识库"
          className="h-11 min-w-0 flex-1 rounded-lg border border-border/60 bg-card px-3.5 text-sm outline-none transition-colors placeholder:text-muted-foreground/60 focus-visible:border-primary/40 focus-visible:ring-2 focus-visible:ring-primary/15"
        />
        <Button
          data-testid="search-submit-button"
          onClick={submit}
          disabled={!trimmed}
          className="h-11 px-4"
        >
          <SearchIcon />
          搜索
        </Button>
      </div>
      {open ? (
        <SearchDropdown state={state} onAskAi={onAskAi ? askAi : undefined} />
      ) : null}
    </div>
  )
}
