import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  server: {
    port: 5173,
    proxy: { '/api': 'http://localhost:3456' },
  },
  build: {
    outDir: 'dist',
    // Wipes dist/, including the tracked dist/.gitkeep that src/http.rs's
    // include_dir! needs on fresh clones; public/.gitkeep copies it back.
    emptyOutDir: true,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    include: ['src/**/*.test.js'],
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
