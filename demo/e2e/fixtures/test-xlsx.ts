/**
 * Test XLSX fixture helper
 *
 * Exports the path to a pre-built minimal xlsx file for upload tests.
 * The file contains a "Sales" sheet with known data for assertions.
 *
 * Content:
 *   Month    | Revenue | Region
 *   January  | 10000   | East
 *   February | 12000   | West
 *   March    | 15000   | North
 *   April    | 11000   | South
 *   May      | 13000   | East
 *
 * Usage:
 * ```typescript
 * import { TEST_XLSX_PATH } from '../fixtures/test-xlsx'
 * await chatPage.uploadFile(TEST_XLSX_PATH)
 * ```
 */

import { fileURLToPath } from 'node:url'
import path from 'node:path'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

/**
 * Absolute path to the test xlsx fixture file.
 */
export const TEST_XLSX_PATH = path.join(__dirname, 'test-data.xlsx')

/**
 * Known data from the test xlsx file, usable for assertions.
 */
export const TEST_XLSX_DATA = {
  sheetName: 'Sales',
  columns: ['Month', 'Revenue', 'Region'],
  rows: [
    { Month: 'January', Revenue: 10000, Region: 'East' },
    { Month: 'February', Revenue: 12000, Region: 'West' },
    { Month: 'March', Revenue: 15000, Region: 'North' },
    { Month: 'April', Revenue: 11000, Region: 'South' },
    { Month: 'May', Revenue: 13000, Region: 'East' },
  ],
  rowCount: 5,
} as const
