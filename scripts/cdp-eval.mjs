// Evaluate JS in the OpenRemote WebView over CDP and print the result.
// Usage: node scripts/cdp-eval.mjs '<js expression>'
import WebSocket from '/home/moreweb/.npm/_npx/32026684e21afda6/node_modules/ws/index.js'

const expr = process.argv[2]
const list = await fetch('http://127.0.0.1:9222/json').then((r) => r.json())
const page = list.find((p) => p.type === 'page')
if (!page) {
  console.error('no page found')
  process.exit(1)
}
const ws = new WebSocket(page.webSocketDebuggerUrl)
ws.on('error', (e) => {
  console.error('ws error:', e.message)
  process.exit(1)
})
ws.on('open', () => {
  ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: expr, awaitPromise: true, returnByValue: true } }))
})
ws.on('message', (data) => {
  const msg = JSON.parse(data)
  if (msg.id === 1) {
    console.log(JSON.stringify(msg.result?.result ?? msg.result, null, 2))
    ws.close()
    process.exit(0)
  }
})
