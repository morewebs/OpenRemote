// One-off: interrogate the running app's webview (CDP 9333) to find why
// the console never auto-connected to the sidecar daemon.
const base = 'http://127.0.0.1:9333'

const connect = async (url) => {
  const ws = new WebSocket(url)
  let seq = 0
  const pending = new Map()
  const send = (method, params = {}) =>
    new Promise((res, rej) => {
      const id = ++seq
      pending.set(id, { res, rej })
      ws.send(JSON.stringify({ id, method, params }))
    })
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data)
    if (m.id && pending.has(m.id)) {
      const p = pending.get(m.id)
      pending.delete(m.id)
      if (m.error) p.rej(new Error(m.error.message))
      else p.res(m.result)
    }
  }
  await new Promise((res, rej) => {
    ws.onopen = res
    ws.onerror = () => rej(new Error('ws connect failed'))
  })
  return { ws, send }
}

const list = await (await fetch(`${base}/json/list`)).json()
const page = list.find((t) => t.type === 'page')
if (!page) throw new Error('no page target found')
const { ws, send } = await connect(page.webSocketDebuggerUrl)

const evalIn = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (r.exceptionDetails) {
    return { threw: r.exceptionDetails.text + ' ' + (r.exceptionDetails.exception?.description ?? '') }
  }
  return r.result.value
}

console.log('location:', await evalIn('location.href'))
console.log('onboarded flag:', await evalIn('localStorage.getItem("openremote-onboarded")'))
console.log('saved url:', await evalIn('localStorage.getItem("openremote-daemon-url")'))
console.log('saved token:', (await evalIn('localStorage.getItem("openremote-daemon-token")'))?.slice(0, 8) + '…')
console.log('has __TAURI_INTERNALS__:', await evalIn('!!window.__TAURI_INTERNALS__'))
console.log('has __TAURI__:', await evalIn('!!window.__TAURI__'))
console.log('csp meta:', await evalIn("document.querySelector('meta[http-equiv=\"Content-Security-Policy\"]')?.content ?? 'none'"))

console.log('\n--- invoking daemon_info directly ---')
console.log(
  await evalIn(`(async () => {
    try {
      const r = await window.__TAURI_INTERNALS__.invoke('daemon_info')
      return JSON.stringify(r)
    } catch (e) {
      return 'THREW: ' + String(e)
    }
  })()`),
)

console.log('\n--- fetching the daemon directly from the page ---')
console.log(
  await evalIn(`(async () => {
    try {
      const url = localStorage.getItem('openremote-daemon-url')
      const token = localStorage.getItem('openremote-daemon-token')
      if (!url) return 'no saved url'
      const r = await fetch(url + '/healthz', { headers: { Authorization: 'Bearer ' + token } })
      return url + ' -> HTTP ' + r.status
    } catch (e) {
      return 'FETCH THREW: ' + String(e)
    }
  })()`),
)

ws.close()
