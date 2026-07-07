import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import {
  LowRecallTable,
  type LowRecallTableProps,
} from '@/components/admin/low-recall-table'
import type {
  LowRecallRecord,
  LowRecallSource,
} from '@/lib/api-generated/types.gen'

/**
 * FE-T02 — LowRecallTable component tests
 *
 * `LowRecallTable` is a fully controlled component (props: `items` / `loading`
 * / `error`; NO pagination props). It has no fetch path, so no MSW is needed.
 * Tests assert the props contract delivered by FE-D03:
 *  - one `low-recall-row` per item
 *  - `topScore` null-vs-number semantic branches (null → 'N/A' miss indicator;
 *    number → renders the numeric score)
 *  - sources expand interaction: clicking the row's source toggle button
 *    reveals the `low-recall-sources` row with each source's
 *    documentId / title / score
 *  - mutual exclusivity of loading / error / empty / list state branches
 *
 * Prop contract note (deviation from FE-T02 item assumptions):
 *  The item allowed for pagination props (`onPrev/onNext/page/total`); FE-D03's
 *  actual `LowRecallTableProps` is `{ items, loading, error }` only, so the
 *  `renderTable` helper omits pagination.
 */

function makeSource(overrides: Partial<LowRecallSource> = {}): LowRecallSource {
  return {
    documentId: crypto.randomUUID(),
    chunkId: 'chunk-1',
    title: 'Source title',
    score: 0.2345,
    ...overrides,
  }
}

let __recordSeq = 0

function makeRecord(
  overrides: Partial<LowRecallRecord> = {},
): LowRecallRecord {
  __recordSeq += 1
  return {
    id: __recordSeq,
    query: `query-${__recordSeq}`,
    resultCount: 0,
    createdAt: '2026-01-01T00:00:00.000Z',
    sources: [],
    siteId: 'site-a',
    ...overrides,
  }
}

function makeRecords(
  count: number,
  overrides: Partial<LowRecallRecord> = {},
): LowRecallRecord[] {
  return Array.from({ length: count }, () => makeRecord({ ...overrides }))
}

function renderTable(overrides: Partial<LowRecallTableProps> = {}) {
  const baseProps: LowRecallTableProps = {
    items: [],
    loading: false,
    error: null,
    ...overrides,
  }
  const user = userEvent.setup()
  const view = render(<LowRecallTable {...baseProps} />)

  function rerender(patch: Partial<LowRecallTableProps>) {
    view.rerender(<LowRecallTable {...baseProps} {...patch} />)
  }

  return { view, user, rerender }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('LowRecallTable — row rendering', () => {
  it('renders one low-recall-row per item', () => {
    renderTable({ items: makeRecords(3) })

    expect(screen.getAllByTestId('low-recall-row')).toHaveLength(3)
  })
})

describe('LowRecallTable — topScore branches', () => {
  // The null-vs-number semantic difference is the load-bearing assertion: when
  // `topScore` is null the cell shows the miss indicator 'N/A'; when it is a
  // number the cell shows the numeric score. We assert the branch divergence,
  // not the exact formatting of either side beyond what distinguishes them.
  it.each([
    {
      label: 'topScore null shows miss indicator (N/A)',
      record: makeRecord({ topScore: null }),
      expectMiss: true,
      expectedNumber: null as number | null,
    },
    {
      label: 'topScore number shows the numeric score',
      record: makeRecord({ topScore: 0.12 }),
      expectMiss: false,
      expectedNumber: 0.12 as number | null,
    },
  ])(
    '$label',
    ({ record, expectMiss, expectedNumber }) => {
      renderTable({ items: [record] })

      // The miss indicator ('N/A') and the numeric score are mutually
      // exclusive — exactly one branch renders per record.
      const missText = screen.queryByText('N/A')
      const numberText =
        expectedNumber !== null
          ? screen.queryByText(expectedNumber.toFixed(4))
          : null

      expect(!!missText).toBe(expectMiss)
      expect(!!numberText).toBe(!expectMiss)
    },
  )
})

describe('LowRecallTable — sources expand interaction', () => {
  it('reveals source details (documentId/title/score) after expanding low-recall-sources', async () => {
    const sources = [
      makeSource({
        documentId: 'doc-alpha',
        chunkId: 'chunk-a',
        title: 'Alpha source',
        score: 0.1111,
      }),
      makeSource({
        documentId: 'doc-beta',
        chunkId: 'chunk-b',
        title: 'Beta source',
        score: 0.2222,
      }),
    ]
    const record = makeRecord({ sources })
    const { user } = renderTable({ items: [record] })

    // Initial state: the expandable `low-recall-sources` row is absent and
    // neither source's documentId is visible.
    expect(screen.queryByTestId('low-recall-sources')).toBeNull()
    expect(screen.queryByText('doc-alpha')).toBeNull()

    // The toggle is the source-title button in the Sources cell of the row.
    // Its accessible name embeds the first source's title plus a `(+N)` count
    // suffix when more than one source exists, so match by prefix.
    await user.click(
      screen.getByRole('button', { name: /Alpha source/ }),
    )

    // After expand: the `low-recall-sources` row appears, containing every
    // source's documentId / title / score (field-passthrough semantics).
    const sourcesRow = await screen.findByTestId('low-recall-sources')
    expect(within(sourcesRow).getByText('doc-alpha')).toBeInTheDocument()
    expect(within(sourcesRow).getByText('doc-beta')).toBeInTheDocument()
    expect(within(sourcesRow).getByText('Alpha source')).toBeInTheDocument()
    expect(within(sourcesRow).getByText('Beta source')).toBeInTheDocument()
    expect(
      within(sourcesRow).getByText(sources[0].score.toFixed(4)),
    ).toBeInTheDocument()
    expect(
      within(sourcesRow).getByText(sources[1].score.toFixed(4)),
    ).toBeInTheDocument()
  })
})

describe('LowRecallTable — state branches', () => {
  // loading short-circuits first, then error, then empty, then the list shell.
  // Each branch is observable via a distinct testid plus the absence of rows.
  it.each([
    {
      label: 'loading shows low-recall-loading and renders no rows',
      overrides: {
        loading: true,
        items: makeRecords(2),
      } as Partial<LowRecallTableProps>,
      expectLoading: true,
      expectError: false,
      expectEmpty: false,
      expectRows: 0,
    },
    {
      label: 'error shows low-recall-error and renders no rows',
      overrides: {
        error: '失败',
        items: makeRecords(2),
      } as Partial<LowRecallTableProps>,
      expectLoading: false,
      expectError: true,
      expectEmpty: false,
      expectRows: 0,
    },
    {
      label: 'empty items shows low-recall-empty and renders no rows',
      overrides: { items: [] } as Partial<LowRecallTableProps>,
      expectLoading: false,
      expectError: false,
      expectEmpty: true,
      expectRows: 0,
    },
  ])(
    '$label',
    ({
      overrides,
      expectLoading,
      expectError,
      expectEmpty,
      expectRows,
    }) => {
      renderTable(overrides)

      expect(!!screen.queryByTestId('low-recall-loading')).toBe(expectLoading)
      expect(!!screen.queryByTestId('low-recall-error')).toBe(expectError)
      expect(!!screen.queryByTestId('low-recall-empty')).toBe(expectEmpty)
      expect(screen.queryAllByTestId('low-recall-row')).toHaveLength(expectRows)
    },
  )
})
