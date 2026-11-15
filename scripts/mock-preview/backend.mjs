import { randomUUID } from 'node:crypto'

// API fixtures only. Rendering always belongs to the actual src/ components.
export const harnesses = [
  ['claude', 'Claude Code'], ['codex', 'Codex'], ['grok', 'Grok'],
  ['pi', 'Pi'], ['opencode', 'OpenCode'], ['agy', 'Antigravity'],
].map(([id, name]) => ({
  id, name, available: true, enabled: true, signed_in: true, version: 'preview',
  honors: { attachments: true, effort: true, model: true },
  permission_modes: [{ w: 'default', l: 'Default' }, { w: 'plan', l: 'Plan' }],
  default_permission: 'default',
}))

export function createMockBackend() {
  let scenario, sessions, projects, automations, agentSettings
  const streams = new Map()
  const timers = new Set()
  function reset(next = 'default') {
    scenario = next
    for (const timer of timers) clearTimeout(timer)
    timers.clear()
    for (const clients of streams.values()) for (const client of clients) client.end()
    streams.clear()
    projects = next === 'empty' ? [] : [
      { id: 'openremote', name: 'OpenRemote', folders: ['/preview/openremote'] },
      { id: 'website', name: 'Website', folders: ['/preview/website'] },
    ]
    sessions = next === 'empty' ? [] : [
      session('design-review', 'Polish the chat composer', 'claude', '/preview/openremote'),
      session('navigation', 'Simplify navigation', 'codex', '/preview/openremote'),
      session('landing-page', 'Explore the landing page', 'grok', '/preview/website'),
    ]
    automations = []
    agentSettings = { harnesses: [], tools: true, max_depth: 2, max_children: 4, max_live: 8, permissions: {} }
    for (const chat of sessions) {
      addEvent(chat, { type: 'message.added', message: { id: randomUUID(), role: 'user', text: chat.title } })
      addEvent(chat, { type: 'message.added', message: { id: randomUUID(), role: 'assistant', text: 'This is sample conversation data in the actual OpenRemote UI. Open the + menu, change models, or send a message to inspect the interaction.' } })
    }
  }
  function session(id, title, harness, workspace) {
    return { id, title, harness, workspace, status: 'idle', created_at: Date.now(), updated_at: Date.now(), events: [], queue: [], permission_mode: 'default' }
  }
  function addEvent(chat, data) {
    const event = { ...data, seq: (chat.events.at(-1)?.seq ?? 0) + 1, at: Date.now() }
    chat.events.push(event)
    chat.updated_at = event.at
    for (const res of streams.get(chat.id) ?? []) res.write(`id: ${event.seq}\ndata: ${JSON.stringify(event)}\n\n`)
  }
  function status(chat, value) {
    chat.status = value
    addEvent(chat, { type: 'session.status_changed', status: value })
  }
  const reply = (res, body, code = 200) => {
    res.writeHead(code, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' })
    res.end(JSON.stringify(body))
  }
  async function handle(req, res) {
    try {
      const url = new URL(req.url, 'http://localhost')
      const path = url.pathname.replace(/^\/devices\/[^/]+/, '')
      const method = req.method
      let body = {}
      if (method !== 'GET' && method !== 'HEAD') {
        let raw = ''
        for await (const part of req) {
          raw += part
          if (raw.length > 75 * 1024 * 1024) return reply(res, { error: 'Preview request too large' }, 413)
        }
        if (raw) body = JSON.parse(raw)
      }
      if (path === '/__preview/reset' && method === 'POST') {
        if (!['default', 'empty', 'error', 'cloud'].includes(body.scenario ?? 'default')) return reply(res, { error: 'Unknown preview scenario' }, 400)
        reset(body.scenario)
        return reply(res, { ok: true, scenario })
      }
      if (scenario === 'error') return reply(res, { error: 'Simulated preview service error' }, 503)
      if (path === '/healthz') return reply(res, { ok: true, version: 'mock-preview' })
      if (path === '/capabilities') return reply(res, { harnesses })
      if (path === '/sessions' && method === 'GET') return reply(res, sessions)
      if (path === '/sessions' && method === 'POST') {
        const chat = session(randomUUID(), null, body.harness, body.workspace || '/preview/openremote')
        Object.assign(chat, { model: body.model, effort: body.effort, permission_mode: body.permission_mode ?? 'default' })
        sessions.unshift(chat)
        return reply(res, chat)
      }
      if (path === '/projects' && method === 'GET') return reply(res, projects)
      if (path === '/projects' && method === 'POST') {
        const project = { id: randomUUID(), name: body.folders?.[0]?.split('/').at(-1) || 'Project', folders: body.folders ?? [] }
        projects.push(project)
        return reply(res, project)
      }
      if (path.startsWith('/projects/') && method === 'DELETE') {
        projects = projects.filter(p => p.id !== path.split('/')[2])
        return reply(res, { ok: true })
      }
      if (path === '/machines') return reply(res, [{ id: 'preview-desktop', name: 'Preview desktop', platform: 'windows', online: true, harnesses }])
      if (path === '/cloud') return reply(res, scenario === 'cloud'
        ? { state: 'online', device: { id: 'preview-desktop', name: 'Preview desktop', kind: 'machine' }, account: { name: 'Alex', email: 'alex@example.test' } }
        : { state: 'signed-out' })
      if (path === '/cloud/devices') return reply(res, { devices: scenario === 'cloud' ? [
        { id: 'preview-desktop', name: 'Preview desktop', kind: 'machine', platform: 'windows', online: true },
        { id: 'preview-phone', name: 'My phone', kind: 'phone', platform: 'android', online: true },
      ] : [] })
      if (path === '/plugins') return reply(res, { builtin: harnesses, installed: [], found: [], global_pending: [] })
      if (['/marketplaces', '/mcp-servers', '/model-endpoints'].includes(path)) return reply(res, [])
      if (path === '/marketplaces/catalog') return reply(res, { entries: [], errors: [] })
      if (path === '/automations' && method === 'GET') return reply(res, automations)
      if (path === '/automations' && method === 'POST') {
        const rule = { ...body, id: body.id ?? randomUUID(), enabled: body.enabled ?? true }
        automations = [...automations.filter(r => r.id !== rule.id), rule]
        return reply(res, rule)
      }
      if (/^\/harnesses\/[^/]+\/models$/.test(path)) {
        const id = path.split('/')[2]
        return reply(res, [{ model: `${id}-sample`, display_name: `${harnesses.find(h => h.id === id)?.name ?? id} sample model`, is_default: true, reasoning_efforts: ['low', 'medium', 'high'] }])
      }
      if (path === '/commands') return reply(res, { commands: [{ name: 'review', description: 'Review changes', kind: 'command', available: true }], agents: [] })
      if (path === '/files') return reply(res, { entries: ['README.md', 'src/App.jsx', 'src/sidebar.css'].filter(p => p.toLowerCase().includes((url.searchParams.get('q') ?? '').toLowerCase())).map(path => ({ path, kind: 'file' })) })
      if (path === '/fs/dirs') return reply(res, { path: url.searchParams.get('path') || '/preview', home: '/preview', parent: '/', dirs: projects.map(p => ({ name: p.name, path: p.folders[0] })) })
      if (path === '/agents/settings') {
        if (method === 'POST') Object.assign(agentSettings, body)
        return reply(res, agentSettings)
      }
      if (path === '/pairing/phones') return reply(res, { phones: [] })
      const match = path.match(/^\/sessions\/([^/]+)(?:\/(.*))?$/)
      if (match) {
        const [, id, action] = match
        const chat = sessions.find(s => s.id === id)
        if (!chat) return reply(res, { error: 'No such preview chat' }, 404)
        if (action === 'events') {
          res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' })
          res.write(': mock preview\n\n')
          const after = Number(url.searchParams.get('after') ?? 0)
          for (const e of chat.events.filter(e => e.seq > after)) res.write(`id: ${e.seq}\ndata: ${JSON.stringify(e)}\n\n`)
          const clients = streams.get(id) ?? new Set()
          clients.add(res)
          streams.set(id, clients)
          const heartbeat = setInterval(() => res.write(': keepalive\n\n'), 10000)
          res.on('close', () => { clearInterval(heartbeat); clients.delete(res) })
          return
        }
        if (!action && method === 'DELETE') { sessions = sessions.filter(s => s.id !== id); return reply(res, { ok: true }) }
        if (!action) return reply(res, chat)
        if (action === 'decisions') return reply(res, [])
        if (action === 'title') chat.title = body.title
        else if (action === 'settings') Object.assign(chat, { model: body.model ?? chat.model, effort: body.effort ?? chat.effort, permission_mode: body.permission_mode ?? chat.permission_mode })
        else if (['stop', 'interrupt'].includes(action)) status(chat, action === 'stop' ? 'stopped' : 'idle')
        else if (action === 'resume') status(chat, 'idle')
        else if (action === 'prompts' || action === 'steer') {
          chat.title ||= body.text?.split('\n')[0]?.slice(0, 44) || 'New chat'
          addEvent(chat, { type: 'message.added', message: { id: randomUUID(), role: 'user', text: body.text || '[Attachment]', turn: chat.events.length } })
          status(chat, 'running')
          const timer = setTimeout(() => {
            timers.delete(timer)
            if (!sessions.includes(chat) || chat.status !== 'running') return
            addEvent(chat, { type: 'message.added', message: { id: randomUUID(), role: 'assistant', text: 'Sample response: your message reached the mock backend. This preview uses the real chat components, menus and styles. Changes to the app update here automatically.', turn: chat.events.length } })
            status(chat, 'idle')
          }, 600)
          timers.add(timer)
        } else return reply(res, { error: `Mock action not implemented: ${action}` }, 501)
        chat.updated_at = Date.now()
        return reply(res, chat)
      }
      return reply(res, { error: `Mock route not implemented: ${method} ${path}` }, 501)
    } catch (error) {
      if (!res.headersSent) reply(res, { error: error.message }, 400)
      else res.end()
    }
  }
  reset()
  return { handle, close: () => reset('empty') }
}
