/**
 * 搜索结果关键词高亮（US-CORE-043）：检索词切成词表，再在标题/摘要中标出命中片段。
 *
 * 切词用 Intl.Segmenter（按词）：中文查询没有空格，按空白切时「部署指南」
 * 只能整串命中，片段里单独出现的「部署」不会被标出。滤词与后端标题加权同口径
 * （`backend/core/src/domain/search.rs` 的 query_keywords）：丢弃单字与纯标点，
 * 否则「的 / a」会让几乎每条结果被标满。
 */

const MIN_TERM_CHARS = 2

export interface HighlightSegment {
  text: string
  match: boolean
}

/** 检索词 → 高亮词表（小写、去重、保序）。 */
export function getHighlightTerms(query: string): string[] {
  // 缺少 Intl.Segmenter 的旧浏览器退化为按空白切词，不让下拉渲染崩溃。
  const words =
    typeof Intl.Segmenter === 'function'
      ? [...new Intl.Segmenter(undefined, { granularity: 'word' }).segment(query)]
          .filter((segment) => segment.isWordLike)
          .map((segment) => segment.segment)
      : query.split(/\s+/)
  const terms = new Set<string>()
  for (const word of words) {
    const term = word.toLowerCase()
    if ([...term].length >= MIN_TERM_CHARS && /[\p{L}\p{N}]/u.test(term)) {
      terms.add(term)
    }
  }
  return [...terms]
}

/**
 * 按词表把文本切成命中/未命中片段：大小写不敏感，同一起点取最长的词。
 * 拉丁词按词级命中（PRD US-CORE-043，同后端 title_matches 口径），见
 * wordPattern。
 */
export function highlightSegments(
  text: string,
  terms: string[],
): HighlightSegment[] {
  if (terms.length === 0) return [{ text, match: false }]
  // 长词在前：正则分支按书写顺序尝试，同一起点优先命中更长的词
  //（rustc 不会被拆成 rust + c）。
  const alternation = [...terms]
    .sort((a, b) => b.length - a.length)
    .map(wordPattern)
    .join('|')
  // split 带捕获组时，命中片段落在奇数下标。
  return text
    .split(new RegExp(`(${alternation})`, 'iu'))
    .map((part, index) => ({ text: part, match: index % 2 === 1 }))
    .filter((segment) => segment.text !== '')
}

/**
 * 词首/词尾是 ASCII 字母数字时加 `\b`："to" 不命中 "tool"、"rust" 不命中
 * "trust"。`\b` 按 ASCII `\w` 判定，中文字符天然算边界——"rust部署" 里的
 * rust 照常命中；中文词本身不加锚定，保持子串命中（「部署」标中「部署流程」）。
 * 不用 Unicode 后行断言：Safari 16.4 之前构造这种正则会直接抛错。
 */
function wordPattern(term: string): string {
  const edge = (char: string) => (/[a-z0-9]/i.test(char) ? '\\b' : '')
  return `${edge(term[0])}${escapeRegExp(term)}${edge(term[term.length - 1])}`
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}
