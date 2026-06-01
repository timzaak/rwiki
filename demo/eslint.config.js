/**
 * ESLint 配置 - E2E 测试专用
 *
 * 核心规则：
 * - 禁止 page.waitForTimeout()（强制使用自动等待）
 * - 未使用变量警告（前缀 _ 除外）
 * - 限制 console 使用（仅允许 warn 和 error）
 */

import eslint from '@eslint/js'
import tsParser from '@typescript-eslint/parser'
import tsPlugin from '@typescript-eslint/eslint-plugin'

export default [
  eslint.configs.recommended,
  {
    files: ['**/*.ts', '**/*.tsx'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2022,
        sourceType: 'module',
        project: './tsconfig.json',
      },
      globals: {
        Buffer: 'readonly',
        process: 'readonly',
        console: 'readonly',
        localStorage: 'readonly',
        sessionStorage: 'readonly',
        fetch: 'readonly',
        setTimeout: 'readonly',
        AbortSignal: 'readonly',
        AbortController: 'readonly',
      },
    },
    plugins: {
      '@typescript-eslint': tsPlugin,
    },
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/explicit-module-boundary-types': 'off',

      'no-restricted-syntax': [
        'error',
        {
          selector: 'CallExpression[callee.object.name="page"][callee.property.name="waitForTimeout"]',
          message: [
            '⛔ Using page.waitForTimeout() is prohibited.',
            '',
            'Alternatives:',
            '  - Use expect().toBeVisible() for element visibility',
            '  - Use waitForLoadState() for page load states',
            '  - Use waitForResponse() for API calls',
            '  - Use waitForURL() for navigation changes',
          ].join('\n'),
        },
      ],

      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'prefer-const': 'error',
      'no-unused-vars': 'off',
    },
  },
  {
    ignores: [
      'node_modules/**',
      'test-results/**',
      'playwright-report/**',
      'playwright/.cache/**',
    ],
  },
]
