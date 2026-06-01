import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import tailwindcss from '@tailwindcss/vite';
import path from 'path';
import { fileURLToPath } from 'url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  plugins: [tailwindcss(), react()],
  resolve: {
    alias: { '@': path.resolve(__dirname, './src') },
  },
  build: {
    target: 'es2022',
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
