// Checks that at full scroll the last transcript message sits above
// the floating composer box. Usage: node scripts/check-clearance.mjs
const base = process.argv[2] ?? 'http://127.0.0.1:9333'

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

const result = await evalIn(`(() => {
  const scroll = document.querySelector('.cv-scroll')
  const box = document.querySelector('.cv-box')
  const thread = document.querySelector('.cv-thread')
  if (!scroll || !box || !thread) return { error: 'chat view not mounted', hash: location.hash }
  scroll.scrollTop = scroll.scrollHeight
  const nodes = [...thread.querySelectorAll('.cv-user, .cv-rail')]
  const last = nodes[nodes.length - 1]
  const scrollRect = scroll.getBoundingClientRect()
  const boxTop = box.getBoundingClientRect().top
  const lastRect = last.getBoundingClientRect()
  return {
    hash: location.hash,
    atFullScroll: scroll.scrollTop + scroll.clientHeight >= scroll.scrollHeight - 1,
    clearVar: getComputedStyle(thread).paddingBottom,
    lastNodeKind: last.className,
    gapAboveBox: +(boxTop - lastRect.bottom).toFixed(1),
    clears: lastRect.bottom <= boxTop,
    scrollHeight: scroll.scrollHeight,
  }
})()`)
console.log(JSON.stringify(result, null, 2))
ws.close()
process.exit(0)
