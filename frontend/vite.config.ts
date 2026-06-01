/**
 * Vite 配置文件
 *
 * 这个文件配置了前端的构建工具和开发服务器。
 * 主要配置项说明：
 *
 * 1. plugins — Vite 插件列表
 *    - tailwindcss()：集成 Tailwind CSS v4，自动处理 CSS
 *    - tanstackRouter()：TanStack Router 的 Vite 插件，
 *      基于文件系统自动生成路由（src/routes/ 目录下的文件）
 *    - react()：支持 JSX 和 React Fast Refresh
 *
 * 2. resolve.alias — 路径别名
 *    - @ → ./src，可以在代码中用 @/components/ui 代替相对路径
 *
 * 3. server.proxy — 开发服务器代理
 *    - /api → 后端服务器（默认 localhost:8080）
 *    这样前端请求 /api/xxx 会自动转发到后端，避免跨域问题
 *
 * 修改指南：
 * - 后端端口变化 → 设置 VITE_API_BASE_URL 环境变量，或修改 proxy.target
 * - 添加新插件 → 在 plugins 数组中添加
 * - 修改端口 → 修改 package.json 中 dev 脚本的 --port 参数
 */
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import tailwindcss from '@tailwindcss/vite'
import { visualizer } from 'rollup-plugin-visualizer'
import path from 'path'
import { fileURLToPath } from 'url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const shouldAnalyze = process.env.ANALYZE === 'true'

export default defineConfig({
  plugins: [
    tailwindcss(),
    tanstackRouter({
      target: 'react',
      autoCodeSplitting: true,
    }),
    react(),
    shouldAnalyze &&
      visualizer({
        filename: 'dist/stats.html',
        gzipSize: true,
        brotliSize: true,
      }),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  optimizeDeps: {
    include: ['@tanstack/react-router'],
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          const normalizedId = id.replaceAll('\\', '/')

          if (
            normalizedId.includes('/node_modules/react/') ||
            normalizedId.includes('/node_modules/react-dom/') ||
            normalizedId.includes('/node_modules/scheduler/')
          ) {
            return 'react-vendor'
          }

          if (
            normalizedId.includes('/node_modules/@tanstack/react-router/') ||
            normalizedId.includes('/node_modules/@tanstack/router-core/') ||
            normalizedId.includes('/node_modules/@tanstack/history/')
          ) {
            return 'router-vendor'
          }

          if (
            normalizedId.includes('/node_modules/react-markdown/') ||
            normalizedId.includes('/node_modules/remark-gfm/') ||
            normalizedId.includes('/node_modules/highlight.js/') ||
            normalizedId.includes('/node_modules/micromark') ||
            normalizedId.includes('/node_modules/unified/') ||
            normalizedId.includes('/node_modules/mdast-util') ||
            normalizedId.includes('/node_modules/hast-util') ||
            normalizedId.includes('/node_modules/remark-parse/') ||
            normalizedId.includes('/node_modules/remark-rehype/') ||
            normalizedId.includes('/node_modules/unist-util') ||
            normalizedId.includes('/node_modules/vfile')
          ) {
            return 'markdown-vendor'
          }
        },
      },
    },
  },
  server: {
    proxy: {
      '/api': {
        target: process.env.VITE_API_BASE_URL || 'http://localhost:8080',
        changeOrigin: true,
        withCredentials: true,
      },
    },
  },
})
