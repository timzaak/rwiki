import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import {
  DocumentTable,
  type DocumentTableProps,
  type DocumentStatusFilter,
} from '@/components/admin/document-table'
import type { DocumentListItem } from '@/lib/api-generated/types.gen'

/**
 * FE-T03 — document-table tests
 *
 * `DocumentTable` is a fully controlled component (no internal data state).
 * These tests assert the props contract from FE-D03:
 *  - row rendering from `documents`
 *  - `onSelectionChange` receives a NEW Set on row toggle / select-all toggle
 *  - `onFilterChange` receives the next filter value
 *  - mutual exclusivity of `loading-spinner` / `error-message` / table shell
 *
 * No MSW is needed: the component has no fetch path. We render directly with
 * `vi.fn()` spies and drive interactions via userEvent.
 */

function makeDoc(overrides: Partial<DocumentListItem> = {}): DocumentListItem {
  return {
    id: crypto.randomUUID(),
    fileName: 'doc.pdf',
    status: 'draft',
    rowCount: 10,
    createdAt: '2026-01-01T00:00:00.000Z',
    errorMessage: null,
    channelId: 'channel-a',
    ...overrides,
  }
}

function makeDocs(
  count: number,
  overrides: Partial<DocumentListItem> = {},
): DocumentListItem[] {
  return Array.from({ length: count }, (_, i) =>
    makeDoc({ ...overrides, fileName: `doc-${i}.pdf` }),
  )
}

function renderTable(overrides: Partial<DocumentTableProps> = {}) {
  const onSelectionChange = vi.fn()
  const onFilterChange = vi.fn()
  const baseProps: DocumentTableProps = {
    documents: [],
    loading: false,
    error: null,
    selectedIds: new Set<string>(),
    onSelectionChange,
    statusFilter: 'all' as DocumentStatusFilter,
    onFilterChange,
    ...overrides,
  }
  const user = userEvent.setup()
  const view = render(<DocumentTable {...baseProps} />)

  // Re-render with merged props, keeping the original spy/handler refs so a
  // controlled prop update (e.g. parent accepting a new selectedIds Set) can
  // be simulated without re-creating the harness.
  function rerender(patch: Partial<DocumentTableProps>) {
    view.rerender(<DocumentTable {...baseProps} {...patch} />)
  }

  return { view, user, onSelectionChange, onFilterChange, rerender }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('DocumentTable — row rendering', () => {
  it('renders one row per document', () => {
    renderTable({ documents: makeDocs(3) })

    expect(screen.getAllByTestId('document-row')).toHaveLength(3)
  })
})

describe('DocumentTable — row selection toggle', () => {
  it('toggles row selection via checkbox (add then remove)', async () => {
    const docs = makeDocs(2)
    const { user, onSelectionChange, rerender } = renderTable({
      documents: docs,
    })

    const checkboxes = screen.getAllByTestId('document-select-checkbox')
    expect(checkboxes).toHaveLength(2)

    // Click the first row's checkbox → onSelectionChange receives a Set
    // containing that row's id (added).
    await user.click(checkboxes[0])

    const firstId = docs[0].id
    expect(onSelectionChange).toHaveBeenCalledTimes(1)
    // Copy into a fresh Set to avoid reference-equality fragility; the spec
    // requires a NEW Set instance on every emission.
    expect(new Set(onSelectionChange.mock.calls.at(-1)![0])).toEqual(
      new Set([firstId]),
    )

    // Simulate the parent accepting the emitted selection (controlled prop),
    // then toggle the same row again → the id is removed.
    const acceptedSelection = new Set(
      onSelectionChange.mock.calls.at(-1)![0] as Set<string>,
    )
    rerender({ selectedIds: acceptedSelection })
    await user.click(screen.getAllByTestId('document-select-checkbox')[0])

    expect(onSelectionChange).toHaveBeenCalledTimes(2)
    expect(new Set(onSelectionChange.mock.calls.at(-1)![0])).toEqual(
      new Set(),
    )
  })
})

describe('DocumentTable — select-all toggle', () => {
  it('select-all adds all visible ids when none selected', async () => {
    const docs = makeDocs(3)
    const { user, onSelectionChange } = renderTable({ documents: docs })

    await user.click(screen.getByTestId('select-all-checkbox'))

    expect(onSelectionChange).toHaveBeenCalledTimes(1)
    expect(new Set(onSelectionChange.mock.calls.at(-1)![0])).toEqual(
      new Set(docs.map((d) => d.id)),
    )
  })

  it('select-all clears when all visible rows already selected', async () => {
    const docs = makeDocs(3)
    const allIds = new Set(docs.map((d) => d.id))
    const { user, onSelectionChange } = renderTable({
      documents: docs,
      selectedIds: allIds,
    })

    await user.click(screen.getByTestId('select-all-checkbox'))

    expect(onSelectionChange).toHaveBeenCalledTimes(1)
    expect(new Set(onSelectionChange.mock.calls.at(-1)![0])).toEqual(new Set())
  })
})

describe('DocumentTable — status filter callback', () => {
  it('emits the chosen filter value on status-filter-select change', async () => {
    const { user, onFilterChange } = renderTable()

    await user.selectOptions(
      screen.getByTestId('status-filter-select'),
      'published',
    )

    expect(onFilterChange).toHaveBeenCalledTimes(1)
    expect(onFilterChange).toHaveBeenCalledWith('published')
  })
})

describe('DocumentTable — state branches', () => {
  // `loading` short-circuits first, then `error`, then the table shell.
  // Each branch is observable via a distinct testid and the absence of rows.
  it.each([
    {
      label: 'loading shows loading-spinner and renders no rows',
      overrides: { loading: true, documents: makeDocs(2) } as Partial<DocumentTableProps>,
      expectSpinner: true,
      expectError: false,
      expectEmpty: false,
      expectRows: 0,
    },
    {
      label: 'error shows error-message and renders no table',
      overrides: { error: '失败', documents: makeDocs(2) } as Partial<DocumentTableProps>,
      expectSpinner: false,
      expectError: true,
      expectEmpty: false,
      expectRows: 0,
    },
    {
      label: 'empty documents shows empty-state',
      overrides: { documents: [] } as Partial<DocumentTableProps>,
      expectSpinner: false,
      expectError: false,
      expectEmpty: true,
      expectRows: 0,
    },
  ])(
    '$label',
    ({ overrides, expectSpinner, expectError, expectEmpty, expectRows }) => {
      renderTable(overrides)

      expect(!!screen.queryByTestId('loading-spinner')).toBe(expectSpinner)
      expect(!!screen.queryByTestId('error-message')).toBe(expectError)
      expect(!!screen.queryByTestId('empty-state')).toBe(expectEmpty)
      expect(screen.queryAllByTestId('document-row')).toHaveLength(expectRows)
    },
  )
})
