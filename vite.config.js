import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    host: true,
    // Pinned to match tauri.conf.json devUrl. Vite's default (5173) collides
    // with other apps on this machine, and its silent fallback to the next
    // free port leaves the Tauri window pointed at the wrong server.
    port: 5175,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**', '**/daemon/**'],
    },
  },
})
