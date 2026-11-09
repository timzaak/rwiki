"use client";

import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogOverlay,
  type SearchItemType,
  type SharedProps,
  useSearch,
} from "fumadocs-ui/components/dialog/search";
import { useI18n } from "fumadocs-ui/contexts/i18n";
import { Sparkles } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { i18n } from "@/lib/i18n";
import {
  apiUrl,
  channelIdForLocale,
  getRWikiChat,
  RWIKI_CHAT_READY_EVENT,
} from "@/lib/rwiki";
import { siteUrl } from "@/lib/seo";

// rwiki POST /api/search 的返回（口径与主站/MCP 一致：page 级排序）。
interface RwikiSearchItem {
  title: string;
  section?: string | null;
  content: string;
  link?: string | null;
  score: number;
  documentId: string;
}

interface RwikiSearchResponse {
  results: RwikiSearchItem[];
}

// 映射后的检索结果条目（结构兼容 fumadocs 的 SearchItemType）。
type ResultItem = {
  id: string;
  url: string;
  type: "page" | "text";
  content: string;
  breadcrumbs?: string[];
  external?: boolean;
};

// 检索状态：error 态直接携带文案，避免 status / errorMessage 两个状态失同步。
type Status =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "error"; message: string };

const STRINGS = {
  en: {
    hint: "Press Enter to search the docs",
    searching: "Searching…",
    failed: "Search failed. Please try again later.",
    empty: "No results found.",
    askPrefix: "Ask AI: ",
  },
  zh: {
    hint: "按 Enter 检索文档",
    searching: "检索中…",
    failed: "搜索失败，请稍后重试",
    empty: "没有找到相关内容",
    askPrefix: "用 AI 回答：",
  },
} as const;

const siteOrigin = new URL(siteUrl).origin;

// 语言段列表与 i18n.languages 同源：新增语言时这里无需同步维护。
const localePrefixPattern = new RegExp(
  `^/(?:${i18n.languages.join("|")})(?=/)`,
);

/**
 * 把 rwiki 结果的 link 解析成文档站内路径。
 * - 生产域名（siteUrl，形如 /docs/x、/zh/docs/x；生产中与站点同源）→ 改写到
 *   当前语言的路径：中文结果页与英文结果页同 slug，跨语言命中也能落地
 * - 与当前站点同源的其余情况（本地预览，origin ≠ siteUrl）→ 保留 pathname/hash，
 *   由 /docs 重定向路由归一化
 * - 其他外链 → 标记 external（新标签页打开）
 */
function resolveLink(
  link: string,
  locale: string,
): { url: string; external?: boolean } | null {
  try {
    const parsed = new URL(link, window.location.origin);
    if (parsed.origin === siteOrigin) {
      // 去掉链接自带的语言段（/zh/docs/x → /docs/x）再拼当前语言，避免 /zh/zh/docs/x。
      const path = parsed.pathname.replace(localePrefixPattern, "");
      return { url: `/${locale}${path}${parsed.hash}` };
    }
    if (parsed.origin === window.location.origin) {
      return { url: `${parsed.pathname}${parsed.hash}` };
    }
    return { url: parsed.toString(), external: true };
  } catch {
    return null;
  }
}

/** page（标题）+ 其下 chunk（章节 + 摘要）两级条目；无 link 的结果（如 OpenAPI 分块）不展示。 */
function mapResults(results: RwikiSearchItem[], locale: string): ResultItem[] {
  const items: ResultItem[] = [];
  const pageIds = new Map<string, string>();
  for (const result of results) {
    if (!result.link) continue;
    const resolved = resolveLink(result.link, locale);
    if (!resolved) continue;

    let pageId = pageIds.get(result.documentId);
    if (!pageId) {
      pageId = `page-${result.documentId}`;
      pageIds.set(result.documentId, pageId);
      items.push({
        id: pageId,
        type: "page",
        url: resolved.url,
        external: resolved.external,
        content: result.title,
      });
    }
    items.push({
      id: `${pageId}-sub-${items.length}`,
      type: "text",
      url: resolved.url,
      external: resolved.external,
      breadcrumbs: result.section ? [result.section] : undefined,
      content: result.content.replace(/\s+/g, " ").trim().slice(0, 280),
    });
  }
  return items;
}

/** 有结果后把默认激活项从「用 AI 回答」挪回第一条结果：再按 Enter 打开的是结果页，不是 LLM。 */
function ActiveItemReset({ items }: { items: SearchItemType[] }) {
  const { setActive } = useSearch();
  useEffect(() => {
    setActive(items.find((item) => item.type !== "action")?.id ?? null);
  }, [items, setActive]);
  return null;
}

export default function DefaultSearchDialog(props: SharedProps) {
  const { locale } = useI18n();
  const lang = locale ?? i18n.defaultLanguage;
  // 文案按语言提供；未提供文案的语言回退英文（频道映射在 rwiki.ts 表驱动）。
  const t = STRINGS[lang as keyof typeof STRINGS] ?? STRINGS.en;
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  // null = 尚未检索（或输入已修改）；非 null = 上一次成功检索的条目。
  const [state, setState] = useState<{
    query: string;
    items: ResultItem[];
  } | null>(null);
  const [search, setSearch] = useState("");
  const abortRef = useRef<AbortController | null>(null);

  const runSearch = useCallback(
    async (query: string) => {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      setStatus({ kind: "loading" });
      try {
        const response = await fetch(`${apiUrl}/api/search`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            query,
            channelId: channelIdForLocale(lang),
          }),
          signal: controller.signal,
        });
        const data = (await response.json().catch(() => null)) as
          | RwikiSearchResponse
          | { message?: string }
          | null;
        // 期间已被新请求/输入修改取代：迟到的响应直接丢弃。
        if (controller.signal.aborted) return;
        if (!response.ok) {
          // 400 的业务文案（如“频道 xxx 未配置”）对部署者有用，原样透出；
          // 其余错误固定文案，不泄露内部细节。
          const message = (data as { message?: string } | null)?.message;
          setStatus({
            kind: "error",
            message: response.status === 400 && message ? message : t.failed,
          });
          return;
        }
        const results = (data as RwikiSearchResponse | null)?.results ?? [];
        setState({ query, items: mapResults(results, lang) });
        setStatus({ kind: "idle" });
      } catch {
        if (controller.signal.aborted) return; // 已被新请求取代
        // 网络/中断等底层失败：固定文案（fetch 的原始报错不面向用户）。
        setStatus({ kind: "error", message: t.failed });
      }
    },
    [lang, t.failed],
  );

  const onSearchChange = useCallback((value: string) => {
    setSearch(value);
    // 修改中的输入不再对应已有结果：中止在途请求并回到“待检索”态，
    // 防止迟到的旧结果覆盖新输入；错误提示一并清掉。
    abortRef.current?.abort();
    setState(null);
    setStatus({ kind: "idle" });
  }, []);

  // 输入框回车触发检索（IME 组合输入不触发）；已有结果时回车交给弹窗
  // 自身的键盘处理（打开当前激活条目），零命中时维持「重新检索」可原词重试
  // ——输入一经修改即回到待检索态。
  const onEnter = useCallback(
    (raw: string) => {
      const query = raw.trim();
      const hasResults = state !== null && state.items.length > 0;
      if (!query || hasResults || status.kind === "loading") return;
      void runSearch(query);
    },
    [state, status, runSearch],
  );

  // SSR 安全：预渲染时 window 不存在，此时不渲染 AI 入口。widget 脚本异步
  // 加载，就绪事件到达后重算，避免渲染期快照过期导致入口整批缺失。
  const [askAvailable, setAskAvailable] = useState(
    () =>
      typeof window !== "undefined" &&
      typeof getRWikiChat()?.ask === "function",
  );
  useEffect(() => {
    const refresh = () =>
      setAskAvailable(typeof getRWikiChat()?.ask === "function");
    window.addEventListener(RWIKI_CHAT_READY_EVENT, refresh);
    return () => window.removeEventListener(RWIKI_CHAT_READY_EVENT, refresh);
  }, []);

  const listItems = useMemo<SearchItemType[] | null>(() => {
    if (!state) return null;
    // 「用 AI 回答」把产生本批结果的检索词交给页面上的 rwiki 聊天 widget；
    // 仅在有结果时出现（零命中时聊天同样没有检索依据，PRD 规定不出现该
    // 入口）；旧版 widget 没有 ask() 时也不渲染。
    if (askAvailable && state.items.length > 0) {
      return [
        {
          id: "ask-ai",
          type: "action" as const,
          node: (
            <div
              className="flex items-center gap-2 overflow-hidden"
              data-testid="docs-search-ask-ai"
            >
              <Sparkles className="size-4 shrink-0 text-fd-primary" />
              <span className="truncate font-medium text-fd-primary">
                {t.askPrefix}
                {state.query}
              </span>
            </div>
          ),
          onSelect: () => {
            getRWikiChat()?.ask?.(state.query);
          },
        },
        ...state.items,
      ];
    }
    return state.items;
  }, [state, askAvailable, t.askPrefix]);

  const statusText =
    status.kind === "loading"
      ? t.searching
      : status.kind === "error"
        ? status.message
        : !state && search.trim()
          ? t.hint
          : null;

  const Empty = useCallback(
    () => (
      <div
        role="status"
        className="py-12 text-center text-sm text-fd-muted-foreground"
      >
        {t.empty}
      </div>
    ),
    [t.empty],
  );

  return (
    <SearchDialog
      search={search}
      onSearchChange={onSearchChange}
      isLoading={status.kind === "loading"}
      {...props}
    >
      <SearchDialogOverlay />
      <SearchDialogContent>
        <SearchDialogHeader>
          <SearchDialogIcon />
          <SearchDialogInput
            data-testid="docs-search-input"
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.keyCode === 229)
                return;
              if (event.key !== "Enter") return;
              onEnter(event.currentTarget.value);
            }}
          />
          <SearchDialogClose />
        </SearchDialogHeader>
        {statusText !== null && (
          <div
            className="border-b px-3 py-2 text-sm text-fd-muted-foreground"
            data-testid="docs-search-status"
          >
            {statusText}
          </div>
        )}
        <SearchDialogList items={listItems} Empty={Empty} />
        {listItems !== null && <ActiveItemReset items={listItems} />}
      </SearchDialogContent>
    </SearchDialog>
  );
}
