/**
 * Vite build for the splash surface. The desktop shell serves these assets
 * only until the dsh web server is ready; the product UI itself is apps/web.
 */

import { defineConfig } from 'vite'

export default defineConfig({
  // Relative asset URLs survive every serving base (tauri://localhost, the
  // vite dev server, and file:// previews).
  base: './',
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
})
