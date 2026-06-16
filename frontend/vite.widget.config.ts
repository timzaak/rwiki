import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [tailwindcss(), react()],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production'),
  },
  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
  },
  build: {
    target: 'es2022',
    // 不清空 outDir：web build（vite.config.ts）与本 widget build 共用 dist/。
    // 默认 emptyOutDir=true 会清掉 dist，导致 widget build 删除 web 的
    // index.html + assets/*。设为 false 让两者共存，后端才能同时 serve
    // web SPA 与 widget（见 docker/Dockerfile、scripts/demo-start.py）。
    emptyOutDir: false,
    chunkSizeWarningLimit: 1200,
    minify: 'terser',
    terserOptions: {
      compress: {
        passes: 2,
      },
    },
    lib: {
      entry: path.resolve(__dirname, 'src/widget/main.tsx'),
      name: 'RWikiChat',
      formats: ['iife'],
      fileName: () => 'rwiki-chat.js',
    },
    rollupOptions: {
      output: {
        extend: true,
        globals: {},
      },
    },
  },
});
