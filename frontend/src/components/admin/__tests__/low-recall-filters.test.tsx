import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'

import {
  LowRecallFilters,
  type LowRecallFiltersProps,
} from '@/components/admin/low-recall-filters'

/**
 * FE-T02 — LowRecallFilters component tests
 *
 * `LowRecallFilters` is a controlled component. Inputs update component-local
 * state on each keystroke; only the Apply (form submit) emits notifications to
 * the parent. There is no fetch path, so no MSW is needed.
 *
 * Prop contract note (deviation from FE-T02 item assumptions — resolved by
 * following the FE-D03 implementation):
 *  The item assumed `onApply(payload)` with a payload object. The actual
 *  contract is:
 *    - four separate controlled-value setters: `onMinScore(number|null)`,
 *      `onMaxScore(number|null)`, `onFrom(string|null)`, `onTo(string|null)`
 *    - a zero-arg `onApply()` invoked last
 *  All five are called exactly once per Apply. The "Apply payload" tests below
 *  therefore assert the four setter payloads + the onApply call, rather than a
 *  single payload object.
 *
 * Numeric tolerance: empty/NaN inputs are normalized to `null` by the
 * component (FE-D03 contract).
 */

function renderFilters(overrides: Partial<LowRecallFiltersProps> = {}) {
  const onMinScore = vi.fn()
  const onMaxScore = vi.fn()
  const onFrom = vi.fn()
  const onTo = vi.fn()
  const onApply = vi.fn()
  const baseProps: LowRecallFiltersProps = {
    minScore: null,
    maxScore: null,
    from: null,
    to: null,
    onMinScore,
    onMaxScore,
    onFrom,
    onTo,
    onApply,
    ...overrides,
  }
  const user = userEvent.setup()
  const view = render(<LowRecallFilters {...baseProps} />)

  function rerender(patch: Partial<LowRecallFiltersProps>) {
    view.rerender(<LowRecallFilters {...baseProps} {...patch} />)
  }

  return {
    view,
    user,
    rerender,
    onMinScore,
    onMaxScore,
    onFrom,
    onTo,
    onApply,
  }
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('LowRecallFilters — controlled inputs', () => {
  it('reflects controlled minScore/maxScore/from/to values in the inputs', () => {
    renderFilters({
      minScore: 0.1,
      maxScore: 0.5,
      from: '2026-01-01T00:00',
      to: '2026-02-01T00:00',
    })

    expect(
      (screen.getByTestId('low-recall-filter-min-score') as HTMLInputElement)
        .value,
    ).toBe('0.1')
    expect(
      (screen.getByTestId('low-recall-filter-max-score') as HTMLInputElement)
        .value,
    ).toBe('0.5')
    expect(
      (screen.getByTestId('low-recall-filter-from') as HTMLInputElement).value,
    ).toBe('2026-01-01T00:00')
    expect(
      (screen.getByTestId('low-recall-filter-to') as HTMLInputElement).value,
    ).toBe('2026-02-01T00:00')
  })
})

describe('LowRecallFilters — Apply notifications', () => {
  it('notifies parent only when Apply is clicked (0 during typing, then 1 on Apply)', async () => {
    const {
      user,
      onMinScore,
      onMaxScore,
      onFrom,
      onTo,
      onApply,
    } = renderFilters()

    // Edit every input. The contract: edits update local state only; no parent
    // notification fires during typing.
    await user.clear(screen.getByTestId('low-recall-filter-min-score'))
    await user.type(
      screen.getByTestId('low-recall-filter-min-score'),
      '0.2',
    )
    await user.clear(screen.getByTestId('low-recall-filter-max-score'))
    await user.type(
      screen.getByTestId('low-recall-filter-max-score'),
      '0.8',
    )
    await user.type(
      screen.getByTestId('low-recall-filter-from'),
      '2026-03-01T00:00',
    )
    await user.type(
      screen.getByTestId('low-recall-filter-to'),
      '2026-04-01T00:00',
    )

    expect(onMinScore).not.toHaveBeenCalled()
    expect(onMaxScore).not.toHaveBeenCalled()
    expect(onFrom).not.toHaveBeenCalled()
    expect(onTo).not.toHaveBeenCalled()
    expect(onApply).not.toHaveBeenCalled()

    // Apply: parent is notified exactly once per callback.
    await user.click(screen.getByTestId('low-recall-apply'))

    expect(onMinScore).toHaveBeenCalledTimes(1)
    expect(onMinScore).toHaveBeenLastCalledWith(0.2)
    expect(onMaxScore).toHaveBeenCalledTimes(1)
    expect(onMaxScore).toHaveBeenLastCalledWith(0.8)
    expect(onFrom).toHaveBeenCalledTimes(1)
    expect(onFrom).toHaveBeenLastCalledWith('2026-03-01T00:00')
    expect(onTo).toHaveBeenCalledTimes(1)
    expect(onTo).toHaveBeenLastCalledWith('2026-04-01T00:00')
    expect(onApply).toHaveBeenCalledTimes(1)
  })

  it('does not call onApply when Apply is not clicked even if inputs change', async () => {
    const { user, onApply } = renderFilters()

    await user.type(
      screen.getByTestId('low-recall-filter-min-score'),
      '0.3',
    )
    await user.type(
      screen.getByTestId('low-recall-filter-from'),
      '2026-05-01T00:00',
    )

    expect(onApply).not.toHaveBeenCalled()
  })
})

describe('LowRecallFilters — empty/invalid input tolerance', () => {
  it('does not block Apply when some inputs are empty (empty fields normalize to null)', async () => {
    const {
      user,
      onMinScore,
      onMaxScore,
      onFrom,
      onTo,
      onApply,
    } = renderFilters()

    // Leave minScore / from empty; fill maxScore / to.
    await user.type(
      screen.getByTestId('low-recall-filter-max-score'),
      '0.9',
    )
    await user.type(
      screen.getByTestId('low-recall-filter-to'),
      '2026-06-01T00:00',
    )
    await user.click(screen.getByTestId('low-recall-apply'))

    // Apply still fires exactly once; empty fields come through as null.
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onMinScore).toHaveBeenLastCalledWith(null)
    expect(onMaxScore).toHaveBeenLastCalledWith(0.9)
    expect(onFrom).toHaveBeenLastCalledWith(null)
    expect(onTo).toHaveBeenLastCalledWith('2026-06-01T00:00')
  })

  it('treats non-numeric minScore as null (NaN normalized away)', async () => {
    const { user, onMinScore, onApply } = renderFilters()

    // 'abc' parses to NaN; the component normalizes NaN to null.
    await user.type(
      screen.getByTestId('low-recall-filter-min-score'),
      'abc',
    )
    await user.click(screen.getByTestId('low-recall-apply'))

    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onMinScore).toHaveBeenLastCalledWith(null)
  })
})
