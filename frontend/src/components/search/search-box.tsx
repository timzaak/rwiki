import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { SearchIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { useSearch } from '@/hooks/use-search'
import { SearchDropdown } from './search-dropdown'

/**
 * 主站频道搜索框：回车或「搜索」按钮触发检索，输入过程零请求。
 * 仅由频道路由 ready 分支挂载，不经 components/chat 共享（Widget 不加载）。
 */
export function SearchBox({ channelId }: { channelId: string }) {
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

  function handleKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key === 'Escape') {
      setOpen(false)
      inputRef.current?.focus()
      return
    }
    if (e.key !== 'Enter' || e.target !== inputRef.current) return
    // Enter confirming an IME composition (isComposing, Safari reports
    // keyCode 229) commits the candidate, not the draft — it must not search.
    if (e.nativeEvent.isComposing || e.keyCode === 229) return
    e.preventDefault()
    submit()
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
      {open ? <SearchDropdown state={state} /> : null}
    </div>
  )
}
