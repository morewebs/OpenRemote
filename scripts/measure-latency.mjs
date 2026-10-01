// Usage: node scripts/measure-latency.mjs [cdpBaseUrl] [ticks] [gapMs] [native]
// 'native' sets the snappy kill switch so the browser's own wheel
// animation is measured instead.
const base = process.argv[2] ?? 'http://127.0.0.1:9333'
const TICKS = Number(process.argv[3] ?? 3)
const GAP = Number(process.argv[4] ?? 60)
const NATIVE = process.argv[5] === 'native'

const list = await (await fetch(`${base}/json/list`)).json()
const page = list.find((t) => t.type === 'page')
if (!page) throw new Error('no page target found')
const ws = new WebSocket(page.webSocketDebuggerUrl)
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
const evalIn = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
  return r.result.value
}

// install a tracker on the chat scroller, then dispatch a 3-tick
// flick through the real input path, then read the result
await evalIn(`(() => {
  window.__lat = null
  const el = document.querySelector('.cv-scroll')
  if (!el) { window.__lat = { error: 'no .cv-scroll' }; return }
  el.scrollTop = 0
  const state = { got: 0, first: 0, last: 0, top: el.scrollTop, ticks: 0, prevented: null, samples: [] }
  const onWheel = (e) => { state.ticks++; if (!state.got) state.got = e.timeStamp }
  el.addEventListener('wheel', onWheel, { passive: true, capture: true })
  // bubble-phase listener added last: sees whether a non-passive
  // listener earlier in this phase called preventDefault
  el.addEventListener('wheel', (e) => { if (state.prevented === null) state.prevented = e.defaultPrevented }, { passive: true })
  const t0 = performance.now()
  const poll = () => {
    const now = performance.now()
    if (state.got && el.scrollTop !== state.top) {
      if (!state.first) state.first = now
      state.last = now
      state.top = el.scrollTop
      state.samples.push([+(now - state.got).toFixed(0), +el.scrollTop.toFixed(1)])
    }
    if (state.first && now - state.last > 300) {
      el.removeEventListener('wheel', onWheel, { capture: true })
      window.__lat = {
        ticks: state.ticks,
        prevented: state.prevented,
        firstMs: +(state.first - state.got).toFixed(1),
        settleMs: +(state.last - state.got).toFixed(1),
        samples: state.samples,
      }
      return
    }
    if (!state.got && now - t0 > 8000) {
      el.removeEventListener('wheel', onWheel, { capture: true })
      window.__lat = { error: 'no wheel event received' }
      return
    }
    requestAnimationFrame(poll)
  }
  requestAnimationFrame(poll)
})()`)

if (NATIVE) await evalIn('window.__snappyOff = true')
else await evalIn('window.__snappyOff = false')

const box = await evalIn(`(function () {
  const el = document.querySelector('.cv-scroll')
  const r = el.getBoundingClientRect()
  return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) }
})()`)

for (let i = 0; i < TICKS; i++) {
  await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: box.x, y: box.y, deltaX: 0, deltaY: 120, deltaMode: 0 })
  await new Promise((r) => setTimeout(r, GAP))
}

const result = await evalIn(`new Promise((resolve) => {
  const c = () => (window.__lat ? resolve(window.__lat) : setTimeout(c, 50))
  c()
})`)
console.log(JSON.stringify(result, null, 2))
ws.close()
process.exit(0)
