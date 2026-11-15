import { createServer } from 'vite'
import { createMockBackend } from './mock-preview/backend.mjs'

// Separate origin/storage and opt-in process; normal dev and production are untouched.
const backend = createMockBackend()
const server = await createServer({
  server: { host: '127.0.0.1', port: 5176, strictPort: true },
  plugins: [{
    name: 'openremote-mock-preview',
    transformIndexHtml: { order: 'pre', handler: () => [{
      tag: 'script', injectTo: 'head-prepend', children: `
        localStorage.setItem('openremote-daemon-url', location.origin + '/__mock');
        localStorage.setItem('openremote-daemon-token', 'mock-preview');
        localStorage.setItem('openremote-onboarded', '1');
      `,
    }] },
    configureServer(vite) {
      vite.middlewares.use((req, res, next) => {
        if (!req.url.startsWith('/__mock/')) return next()
        req.url = req.url.slice('/__mock'.length)
        return backend.handle(req, res)
      })
    },
  }],
})
await server.listen()
console.log('\nOpenRemote mock preview: http://127.0.0.1:5176/#new\nReal UI + Vite hot reload. Sample API data only. No daemon or agents.\n')
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  backend.close()
  await server.close()
  process.exit(0)
})
