/**
 * Test XLSX fixture helper
 *
 * Exports the path to a pre-built minimal xlsx file for upload tests.
 * The file uses the Wiki-style columns required by
 * `backend/core/src/infrastructure/xlsx_parser.rs` (`Title` + `Markdown Content`
 * are mandatory; `Locale`, `Link`, `Tags` are optional). The `Monthly Revenue
 * Report` row is intentionally seeded with a markdown body whose content
 * answers the chat-rag-streaming RAG question ("Revenue 最高的月份是哪个?").
 *
 * Content (sheet "Sales"):
 *   Title                  | Markdown Content                          | Locale | Link | Tags
 *   Monthly Revenue Report | # 月度营收报告 … Revenue 最高的是 March | zh-CN  |      | sales,revenue
 *   Product Overview       | # Product Overview …                      | en     |      | product
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
 * Known titles authored in the test xlsx file, usable for assertions.
 */
export const TEST_XLSX_DATA = {
  sheetName: 'Sales',
  columns: ['Title', 'Markdown Content', 'Locale', 'Link', 'Tags'],
  titles: ['Monthly Revenue Report', 'Product Overview'],
  rowCount: 2,
} as const
