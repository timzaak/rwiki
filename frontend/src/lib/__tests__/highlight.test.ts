import { describe, it, expect } from 'vitest'

import { getHighlightTerms, highlightSegments } from '@/lib/highlight'

/**
 * US-CORE-043 — 搜索结果高亮的切词与匹配契约。
 *
 * WHY 这些用例存在：
 *  - 中文查询没有空格：退化为按空白切词时「部署指南」只能整串命中，
 *    片段里单独出现的「部署」永远不亮。
 *  - 单字、纯标点不高亮：与后端标题加权同一滤词口径，否则「的 / a」
 *    会让几乎每条结果被标满，高亮失去指向性。
 *  - 查询词按字面匹配：正则元字符未转义时「v1.2」会误标「v1x2」，
 *    含括号的查询甚至会抛错让整个下拉崩溃。
 *  - 拉丁词按词边界命中（同后端 title_matches 口径）：无锚定时 "to" 会
 *    标中 "tool"、"rust" 会标中 "trust"，标题被虚假高亮刷成噪音；中文词
 *    无词边界概念，保持子串命中。
 */

function marked(text: string, query: string): string[] {
  return highlightSegments(text, getHighlightTerms(query))
    .filter((segment) => segment.match)
    .map((segment) => segment.text)
}

describe('getHighlightTerms', () => {
  it('splits a space-less Chinese query into words', () => {
    expect(getHighlightTerms('部署指南')).toEqual(['部署', '指南'])
  })

  it('drops single-character and punctuation-only segments', () => {
    expect(getHighlightTerms('的 a 部署！')).toEqual(['部署'])
  })

  it('lowercases and dedupes terms in first-seen order', () => {
    expect(getHighlightTerms('Rust 部署 rust RUST')).toEqual(['rust', '部署'])
  })

  it('returns no terms for a blank query', () => {
    expect(getHighlightTerms('   ')).toEqual([])
  })
})

describe('highlightSegments', () => {
  it('marks Chinese query words found separately in the text', () => {
    expect(marked('本文介绍部署流程与配置指南', '部署指南')).toEqual(['部署', '指南'])
  })

  it('matches case-insensitively and keeps the original casing', () => {
    expect(marked('Rust 与 RUST 教程', 'rust')).toEqual(['Rust', 'RUST'])
  })

  it('prefers the longest term at the same position', () => {
    expect(marked('install rustc first', 'rust rustc')).toEqual(['rustc'])
  })

  it('matches Latin terms on word boundaries only', () => {
    expect(marked('This tool installs items', 'to it')).toEqual([])
    expect(marked('trust the rust guide', 'rust')).toEqual(['rust'])
    // 边界按词首/词尾字符判定：带标点的版本号同样不得命中更长的版本号。
    expect(marked('升级到 v1.23 而非 v1.2', 'v1.2')).toEqual(['v1.2'])
  })

  it('still matches CJK terms as substrings across boundaries', () => {
    expect(marked('rust部署流程指南', 'rust 部署')).toEqual(['rust', '部署'])
  })

  it('treats regex metacharacters in the query literally', () => {
    expect(marked('v1x2 已废弃，请升级到 v1.2', 'v1.2')).toEqual(['v1.2'])
    expect(() => highlightSegments('beta (预览)', ['(预览'])).not.toThrow()
  })

  it('keeps every character of the original text across segments', () => {
    const text = 'Getting Started：部署 getting started'
    const segments = highlightSegments(text, getHighlightTerms('getting started 部署'))
    expect(segments.map((segment) => segment.text).join('')).toBe(text)
  })

  it('returns the text as one unmatched segment when there are no terms', () => {
    expect(highlightSegments('任意文本', [])).toEqual([
      { text: '任意文本', match: false },
    ])
  })
})
