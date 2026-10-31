// The web console's build: the same React sources as the desktop, a different
// entry (no shell, no daemon sidecar) and its own output folder, so the
// desktop bundle and its Tauri configuration are untouched.
//
// `base: '/app/'` is where the shipped page lives; a dev or preview server
// serves the same tree under that path, which keeps asset URLs identical
// between the two.
import { renameSync } from 'node:fs'
import { join } from 'node:path'

import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// A static host answers the address with the trailing slash from the folder's
// `index.html`, so the page a person opens at `/app/` has to carry that name.
// The source keeps its own name because the desktop build owns `index.html` at
// the repository root, and only the file is renamed: `base` already makes every
// asset URL absolute, so nothing inside the page moves.
function indexAtTheSiteRoot() {
  return {
    name: 'index-at-the-site-root',
    apply: 'build',
    enforce: 'post',
    writeBundle(options, bundle) {
      const out = options.dir
      if (!out) return
      const page = Object.keys(bundle).find((name) => name.endsWith('.html'))
      if (!page || page === 'index.html') return
      renameSync(join(out, page), join(out, 'index.html'))
    },
  }
}

export default defineConfig({
  base: '/app/',
  plugins: [react(), indexAtTheSiteRoot()],
  build: {
    outDir: 'dist-web',
    emptyOutDir: true,
    rollupOptions: {
      input: 'web.html',
    },
  },
  server: {
    host: '127.0.0.1',
    port: 5176,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**', '**/daemon/**', '**/.*.tmpdir/**'],
      usePolling: true,
      interval: 400,
    },
  },
  preview: {
    host: '127.0.0.1',
    port: 5177,
    strictPort: true,
  },
})
