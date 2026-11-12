// The bridge inside every JavaScript plugin's frame. The console loads it
// first, before any of the plugin's own code, into a frame sandboxed to
// scripts alone: no same origin, so the frame can read nothing of the
// console, and a policy that lets it reach no address of its own.
//
// It gives the plugin `window.openremote` - calls carried to the console
// over postMessage, each numbered and answered - then asks the console for
// the plugin's page and builds it here: the markup and styles, pictures as
// data: URLs, and the scripts, in order, as blob: scripts made inside this
// frame (the only scripts the frame's policy runs besides this one).
//
// It never holds the daemon's token or address: every call it makes is
// answered by the console, which asks the daemon itself.
;(() => {
  'use strict'
  const host = window.parent
  if (!host || host === window) return

  const pending = new Map()
  const clickHandlers = new Set()
  const settingsHandlers = new Set()
  const waitingClicks = []
  let next = 1
  let booted = false
  let info = null

  const send = (message, transfer) => host.postMessage({ openremote: 1, ...message }, '*', transfer ?? [])

  function call(method, params = {}, transfer) {
    return new Promise((resolve, reject) => {
      const id = next++
      pending.set(id, { resolve, reject })
      send({ kind: 'call', id, method, params }, transfer)
    })
  }

  function report(problem) {
    const text = String(problem?.message ?? problem)
    send({ kind: 'log', text })
  }

  function safely(handler, detail) {
    try {
      const result = handler(detail)
      if (result && typeof result.catch === 'function') result.catch(report)
    } catch (err) {
      report(err)
    }
  }

  // ---- files: the plugin's own, read through the console ----

  const local = (url) => typeof url === 'string' && url !== '' && !/^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(url)

  // A path the page names, against the folder of the page that names it.
  function resolve(from, path) {
    const folder = from.includes('/') ? from.slice(0, from.lastIndexOf('/') + 1) : ''
    const url = new URL(path, `https://plugin.invalid/${folder}`)
    return decodeURIComponent(url.pathname.slice(1))
  }

  function base64(bytes) {
    const view = new Uint8Array(bytes)
    let binary = ''
    for (let i = 0; i < view.length; i += 0x8000) binary += String.fromCharCode.apply(null, view.subarray(i, i + 0x8000))
    return btoa(binary)
  }

  const readFile = (path) => call('files.read', { path })

  async function readText(path) {
    const { bytes } = await readFile(path)
    return new TextDecoder().decode(bytes)
  }

  async function readDataUrl(path) {
    const { bytes, type } = await readFile(path)
    return `data:${(type || 'application/octet-stream').split(';')[0]};base64,${base64(bytes)}`
  }

  // ---- the page ----

  function applyTheme(tokens) {
    const vars = Object.entries(tokens ?? {})
      .filter(([name, value]) => /^--[a-z0-9-]+$/.test(name) && typeof value === 'string' && !/[;{}<]/.test(value))
      .map(([name, value]) => `${name}: ${value};`)
      .join(' ')
    const style = document.createElement('style')
    style.setAttribute('data-openremote', 'base')
    style.textContent =
      `:root { color-scheme: dark; ${vars} } ` +
      'html, body { margin: 0; min-height: 100%; background: var(--bg-app); color: var(--text-1); ' +
      'font: 13px/1.5 var(--font-sans, system-ui, sans-serif); } ' +
      '*, *::before, *::after { box-sizing: border-box; }'
    document.head.prepend(style)
  }

  function applyMotion(reduce) {
    if (reduce) document.documentElement.setAttribute('data-reduce-motion', '')
    else document.documentElement.removeAttribute('data-reduce-motion')
  }

  // A page that could not be built: said in the frame (a view shows it)
  // and to the console (which tells the user when no view would).
  function showProblem(words) {
    send({ kind: 'failed', text: String(words) })
    const note = document.createElement('p')
    note.textContent = words
    note.style.cssText = 'margin: 32px; color: var(--text-3); font-size: 13px;'
    document.body.replaceChildren(note)
  }

  // One of the plugin's scripts, run as a blob: script made here. The
  // sourceURL names the file in the developer tools.
  function runScript(code, label, module) {
    return new Promise((done) => {
      const name = String(label).replace(/[\r\n]/g, ' ')
      const url = URL.createObjectURL(new Blob([`${code}\n//# sourceURL=${name}`], { type: 'text/javascript' }))
      const script = document.createElement('script')
      if (module) script.type = 'module'
      script.async = false
      script.src = url
      script.onload = () => {
        URL.revokeObjectURL(url)
        done()
      }
      script.onerror = () => {
        URL.revokeObjectURL(url)
        report(`${name} did not run`)
        done()
      }
      document.body.append(script)
    })
  }

  // An HTML page: its scripts set aside, its linked stylesheets and
  // pictures read in, its markup in place - then the scripts, in order.
  async function buildPage(html, path) {
    const page = new DOMParser().parseFromString(html, 'text/html')
    const scripts = []
    for (const script of page.querySelectorAll('script')) {
      scripts.push({ src: script.getAttribute('src'), type: (script.getAttribute('type') ?? '').trim(), code: script.textContent ?? '' })
      script.remove()
    }
    for (const link of page.querySelectorAll('link[rel~="stylesheet"][href]')) {
      const href = link.getAttribute('href')
      const style = page.createElement('style')
      if (local(href)) {
        try {
          style.textContent = await readText(resolve(path, href))
        } catch (err) {
          report(err)
        }
      } else {
        report(`left out ${href}: a plugin's styles come from its own folder`)
      }
      link.replaceWith(style)
    }
    for (const element of page.querySelectorAll('[src], video[poster]')) {
      for (const name of ['src', 'poster']) {
        const value = element.getAttribute(name)
        if (!value || !local(value)) continue
        try {
          element.setAttribute(name, await readDataUrl(resolve(path, value)))
        } catch (err) {
          report(err)
        }
      }
    }
    if (page.title) document.title = page.title
    for (const node of [...page.head.childNodes]) document.head.append(document.adoptNode(node))
    for (const attr of [...page.body.attributes]) document.body.setAttribute(attr.name, attr.value)
    for (const node of [...page.body.childNodes]) document.body.append(document.adoptNode(node))
    for (const script of scripts) {
      const module = /^module$/i.test(script.type)
      // A data block (JSON, a template) is not a script to run.
      if (script.type && !module && !/^(text|application)\/(java|ecma)script$/i.test(script.type)) continue
      if (script.src && !local(script.src)) {
        report(`left out ${script.src}: a plugin's scripts come from its own folder`)
        continue
      }
      try {
        const file = script.src ? resolve(path, script.src) : path
        const code = script.src ? await readText(file) : script.code
        await runScript(code, file, module)
      } catch (err) {
        report(err)
      }
    }
  }

  async function boot(message) {
    info = { plugin: message.plugin, place: message.place }
    applyTheme(message.theme)
    applyMotion(message.reduceMotion)
    const entry = message.entry?.path
    if (!entry) {
      showProblem(message.error ?? 'This plugin has no page.')
      return
    }
    let text
    try {
      text = await readText(entry)
    } catch (err) {
      showProblem(err?.message ?? String(err))
      return
    }
    if (/\.html?$/i.test(entry)) await buildPage(text, entry)
    else await runScript(text, entry, /\.mjs$/i.test(entry))
    // A plugin written for a page that loads the usual way waits for
    // these; here its scripts run after the page is in place.
    document.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }))
    window.dispatchEvent(new Event('load'))
    send({ kind: 'booted' })
  }

  function deliver(message) {
    if (message.type === 'composer.click') {
      if (!clickHandlers.size) {
        waitingClicks.push(message.detail)
        return
      }
      for (const handler of clickHandlers) safely(handler, message.detail)
    } else if (message.type === 'settings.changed') {
      for (const handler of settingsHandlers) safely(handler, message.detail)
    } else if (message.type === 'motion') {
      applyMotion(message.detail?.reduce)
    }
  }

  // ---- fetch, through the console and the daemon ----

  const NULL_BODY = new Set([101, 103, 204, 205, 304])

  async function pluginFetch(input, init = {}) {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input?.url
    if (!url) throw new TypeError('openremote.fetch needs an address')
    const method = String(init.method ?? 'GET').toUpperCase()
    const headers = new Headers(init.headers ?? {})
    let body = null
    if (init.body != null) {
      if (typeof ReadableStream !== 'undefined' && init.body instanceof ReadableStream) {
        throw new TypeError('openremote.fetch takes a body of text, bytes, a Blob or FormData, not a stream')
      }
      // A Request works out the bytes and, for FormData, the multipart
      // type with its boundary - the same body fetch itself would send.
      const shaped = new Request('https://plugin.invalid/', { method: 'POST', body: init.body, headers })
      body = await shaped.arrayBuffer()
      const type = shaped.headers.get('content-type')
      if (type && !headers.has('content-type')) headers.set('content-type', type)
    }
    const signal = init.signal
    if (signal?.aborted) throw new DOMException('The request was stopped', 'AbortError')
    const asked = call('fetch', { url: String(url), method, headers: [...headers], body, timeoutMs: init.timeoutMs ?? null }, body ? [body] : [])
    const reply = await (signal
      ? Promise.race([
          asked,
          new Promise((_, reject) => signal.addEventListener('abort', () => reject(new DOMException('The request was stopped', 'AbortError')), { once: true })),
        ])
      : asked)
    // A Response takes 200-599; anything else from a server is a broken
    // answer, and reads as one.
    const status = reply.status >= 200 && reply.status <= 599 ? reply.status : 502
    return new Response(NULL_BODY.has(status) || method === 'HEAD' ? null : reply.body, {
      status,
      headers: reply.headers,
    })
  }

  // ---- the plugin's API ----

  const api = {
    version: 1,
    get plugin() {
      return info?.plugin ?? null
    },
    // 'background' (a hidden frame, for composer buttons) or 'view'.
    get place() {
      return info?.place?.kind ?? null
    },
    get view() {
      return info?.place?.view ?? null
    },
    composer: {
      insert: (text, options = {}) => call('composer.insert', { text: String(text ?? ''), composer: options.composer ?? null }),
      async attach(file, options = {}) {
        const blob = file instanceof Blob ? file : new Blob([file], { type: options.type ?? '' })
        const bytes = await blob.arrayBuffer()
        return call(
          'composer.attach',
          { name: options.name ?? file?.name ?? 'attachment', mime: blob.type || options.type || '', bytes, composer: options.composer ?? null },
          [bytes],
        )
      },
      setState: (input, state = {}, options = {}) =>
        call('composer.setState', {
          input,
          state: state.state ?? 'idle',
          label: state.label ?? null,
          title: state.title ?? null,
          composer: options.composer ?? null,
        }),
      onClick(handler) {
        clickHandlers.add(handler)
        while (waitingClicks.length) {
          const detail = waitingClicks.shift()
          for (const each of clickHandlers) safely(each, detail)
        }
        return () => clickHandlers.delete(handler)
      },
    },
    chats: {
      list: () => call('chats.list'),
      read: (id) => call('chats.read', { id }),
      start: (options = {}) =>
        call('chats.start', {
          text: options.text ?? '',
          harness: options.harness ?? null,
          workspace: options.workspace ?? null,
          model: options.model ?? null,
        }),
      steer: (id, text) => call('chats.steer', { id, text: String(text ?? '') }),
      stop: (id) => call('chats.stop', { id }),
    },
    fetch: pluginFetch,
    settings: {
      get: () => call('settings.get'),
      onChange(handler) {
        settingsHandlers.add(handler)
        return () => settingsHandlers.delete(handler)
      },
    },
    ui: {
      toast: (text) => call('ui.toast', { text: String(text ?? '') }),
      openView: (view) => call('ui.openView', { view: view ?? null }),
      openSettings: () => call('ui.openSettings'),
    },
    files: {
      async read(path, options = {}) {
        const { bytes, type } = await readFile(path)
        const as = options.as ?? 'text'
        if (as === 'arraybuffer') return bytes
        if (as === 'blob') return new Blob([bytes], { type })
        if (as === 'dataurl') return `data:${(type || 'application/octet-stream').split(';')[0]};base64,${base64(bytes)}`
        return new TextDecoder().decode(bytes)
      },
    },
  }

  Object.defineProperty(window, 'openremote', { value: Object.freeze(api), writable: false, configurable: false })

  window.addEventListener('message', (event) => {
    if (event.source !== host) return
    const message = event.data
    if (!message || message.openremote !== 1) return
    if (message.kind === 'reply') {
      const waiting = pending.get(message.id)
      if (!waiting) return
      pending.delete(message.id)
      if (message.error != null) waiting.reject(new Error(message.error))
      else waiting.resolve(message.result)
    } else if (message.kind === 'boot' && !booted) {
      booted = true
      boot(message).catch((err) => {
        report(err)
        showProblem(`This plugin's page did not load: ${err?.message ?? err}`)
      })
    } else if (message.kind === 'event') {
      deliver(message)
    }
  })

  window.addEventListener('error', (event) => report(event.message || 'a script failed'))
  window.addEventListener('unhandledrejection', (event) => report(event.reason ?? 'a promise failed'))

  // The app's keys keep working while a plugin's view has focus: a key
  // with Ctrl, Cmd or Alt goes up to the console - unless the plugin took
  // it, or the user is typing in one of the page's own fields (AltGr
  // characters ride Ctrl+Alt on Windows).
  window.addEventListener('keydown', (event) => {
    if (event.defaultPrevented || !(event.ctrlKey || event.metaKey || event.altKey)) return
    if (['Control', 'Meta', 'Alt', 'Shift', 'AltGraph'].includes(event.key)) return
    if (event.getModifierState?.('AltGraph')) return
    const target = event.target
    if (target instanceof Element && target.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"]')) return
    send({
      kind: 'key',
      key: event.key,
      code: event.code,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      shiftKey: event.shiftKey,
      repeat: event.repeat,
    })
  })

  // Say hello until the console answers: the frame can be up a moment
  // before the console has noted it.
  let tries = 0
  const knock = setInterval(() => {
    if (booted || ++tries > 25) clearInterval(knock)
    else send({ kind: 'ready' })
  }, 400)
  send({ kind: 'ready' })
})()
