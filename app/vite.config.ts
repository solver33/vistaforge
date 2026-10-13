import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// The same policy the Tauri window enforces (minus its IPC origins), so browser tests catch
// anything that would be blocked there, such as inline style attributes.
const csp = "default-src 'self'; img-src 'self' data: blob:; style-src 'self'";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // `npm run dev:real` serves the commands from the real core here.
    proxy: { '/api': 'http://127.0.0.1:1421' },
  },
  preview: {
    port: 4173,
    strictPort: true,
    headers: { 'Content-Security-Policy': csp },
  },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 1500,
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
});
