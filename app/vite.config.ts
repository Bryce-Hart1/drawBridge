/// <reference types="vitest/config" />
// Claude — Date 11/03/2026
// Tauri loads this dev server from 127.0.0.1:1420 (`devUrl` in
// src-tauri/tauri.conf.json). A plain browser pointed at the same address gets
// the mock backend instead (src/lib/mock.ts), for working on the UI alone.

import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  // WKWebView on macOS 13 and later.
  build: { target: 'safari16', outDir: 'dist', emptyOutDir: true },
  test: { include: ['src/**/*.test.ts'] },
});
