// Measures real frame rate in the OpenRemote webview via CDP.
// Usage: node scripts/measure-fps.mjs [cdpBaseUrl]
const base = process.argv[2] ?? 'http://127.0.0.1:9333'

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

// run an in-page async expression, return its value
const evalIn = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text)
  return r.result.value
}

const countExpr = `(ms) => new Promise((res) => {
  let n = 0; const t0 = performance.now()
  const f = () => { n++; if (performance.now() - t0 < ms) requestAnimationFrame(f); else res(Math.round(n / ((performance.now() - t0) / 1000))) }
  requestAnimationFrame(f)
})`

const results = {}
results.idle = await evalIn(`(${countExpr})(1500)`)

// programmatic continuous scroll (paint cost of the transcript)
results.progScroll = await evalIn(`(async () => {
  const el = document.querySelector('.cv-scroll')
  if (!el) return null
  el.scrollTop = 0
  let n = 0; const t0 = performance.now()
  await new Promise((res) => {
    const step = () => {
      n++
      el.scrollTop = (el.scrollTop + 16) % (el.scrollHeight - el.clientHeight)
      if (performance.now() - t0 < 1500) requestAnimationFrame(step); else res()
    }
    requestAnimationFrame(step)
  })
  return Math.round(n / 1.5)
})()`)

// scrollbar experiment: hide custom scrollbars, re-measure, restore
results.progScrollNoCustomSb = await evalIn(`(async () => {
  const el = document.querySelector('.cv-scroll')
  if (!el) return null
  const s = document.createElement('style')
  s.textContent = '*::-webkit-scrollbar { display: none } .cv-scroll, .sb-list { scrollbar-width: none }'
  document.head.appendChild(s)
  await new Promise((r) => setTimeout(r, 100))
  let n = 0; const t0 = performance.now()
  await new Promise((res) => {
    const step = () => {
      n++
      el.scrollTop = (el.scrollTop + 16) % (el.scrollHeight - el.clientHeight)
      if (performance.now() - t0 < 1500) requestAnimationFrame(step); else res()
    }
    requestAnimationFrame(step)
  })
  s.remove()
  return Math.round(n / 1.5)
})()`)

// real input path: dispatch actual wheel events through CDP, count fps
const box = await evalIn(`(function () {
  const el = document.querySelector('.cv-scroll')
  if (!el) return null
  const r = el.getBoundingClientRect()
  return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2), h: el.clientHeight, sh: el.scrollHeight }
})()`)
if (box) {
  await evalIn(`document.querySelector('.cv-scroll').scrollTop = 0`)
  const wheelStart = Date.now()
  const spin = setInterval(() => {
    send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: box.x, y: box.y, deltaY: 120, deltaMode: 0 }).catch(() => {})
    if (Date.now() - wheelStart > 1500) clearInterval(spin)
  }, 30)
  results.wheelScroll = await evalIn(`(${countExpr})(1600)`)
  clearInterval(spin)
}

results.view = await evalIn('location.hash')
results.dpr = await evalIn('devicePixelRatio')
console.log(JSON.stringify(results, null, 2))

// GPU info from the browser-level target
try {
  const ver = await (await fetch(`${base}/json/version`)).json()
  const b = await connect(ver.webSocketDebuggerUrl)
  const gpu = await b.send('SystemInfo.getInfo')
  console.log('adapters:', JSON.stringify(gpu.gpu.adapters.map((a) => ({ desc: (a.description || '').slice(0, 48), active: a.active ?? true }))))
  b.ws.close()
} catch (e) {
  console.log('gpu info unavailable:', e.message)
}

ws.close()
process.exit(0)
