/**
 * ESLint 配置 - 前端应用（src/）
 *
 * 核心规则：
 * - 未使用变量报错（前缀 _ 除外）
 * - 限制 console 使用（仅允许 warn 和 error）
 * - api-generated/ 为 openapi-ts 生成代码，整体忽略
 */

import eslint from '@eslint/js'
import tsParser from '@typescript-eslint/parser'
import tsPlugin from '@typescript-eslint/eslint-plugin'

export default [
  {
    ignores: ['dist/**', 'node_modules/**', 'src/lib/api-generated/**'],
  },
  eslint.configs.recommended,
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      parser: tsParser,
      parserOptions: {
        ecmaVersion: 2022,
        sourceType: 'module',
      },
    },
    plugins: {
      '@typescript-eslint': tsPlugin,
    },
    rules: {
      // 未定义变量检查由 tsc --noEmit（CI 门禁）承担；
      // no-undef 不识别 TS 类型导入，在 TS 文件上会误报。
      'no-undef': 'off',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/explicit-function-return-type': 'off',
      '@typescript-eslint/explicit-module-boundary-types': 'off',

      'no-console': ['warn', { allow: ['warn', 'error'] }],
      'prefer-const': 'error',
      'no-unused-vars': 'off',
    },
  },
]
