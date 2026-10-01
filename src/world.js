// The local picture of an OpenRemote workspace: machines you own,
// harnesses signed in on them, sessions, plugins, and automations.
// Nothing here talks to a real process.

import { normalizeTrigger, whenSentence } from './automation.js'
import { AUTOMATION_STARTERS, PLUGINS } from './library.js'

export const WORLD_KEY = 'openremote-ui-world'

export const PROJECTS = [
  { id: 'webapp', name: 'webapp', path: '~/src/webapp' },
  { id: 'api', name: 'api', path: '~/src/api' },
  { id: 'payments', name: 'payments', path: '~/src/payments' },
  { id: 'site', name: 'site', path: '~/src/site' },
]

const HARNESS_NAMES = {
  cc: 'Claude Code',
  cx: 'Codex',
  od: 'OpenCode',
  pi: 'Pi Agent',
  amp: 'Amp',
  gem: 'Gemini CLI',
  grok: 'Grok Build',
  cur: 'Cursor Agent',
}

const OS = {
  macos: { platform: 'macOS', kind: 'desktop', spec: 'macOS' },
  linux: { platform: 'Linux', kind: 'desktop', spec: 'Linux' },
  windows: { platform: 'Windows', kind: 'laptop', spec: 'Windows' },
}

function nid(prefix) {
  const rand = globalThis.crypto?.randomUUID?.() ?? Math.random().toString(16).slice(2)
  return `${prefix}-${rand}`
}

export function contextWindow(modelId) {
  return modelId === 'sonnet-1m' ? 1_000_000 : 200_000
}

export function formatTokens(n) {
  if (n >= 1_000_000) {
    const m = n / 1_000_000
    return Number.isInteger(m) ? `${m}M` : `${m.toFixed(1)}M`
  }
  if (n >= 1000) return `${Math.round(n / 1000)}k`
  return String(Math.max(0, Math.round(n)))
}

export function ringGeometry(used, window) {
  const circumference = 2 * Math.PI * 8
  const frac = window <= 0 ? 0 : Math.min(1, Math.max(0, used / window))
  return {
    circumference,
    offset: circumference * (1 - frac),
    pct: Math.round(frac * 100),
    remaining: Math.max(0, window - used),
  }
}

export function usageFor(text) {
  return Math.max(800, Math.ceil(String(text ?? '').trim().length / 4) * 120)
}

function titleFrom(text) {
  const first = String(text ?? '').trim().split('\n')[0]
  return first.length > 44 ? `${first.slice(0, 44).trimEnd()}…` : first
}

function slug(name) {
  return String(name ?? '')
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
}

function chat(partial) {
  return {
    messages: [],
    contextUsed: 12000,
    compactQueued: false,
    approvals: [],
    model: 'sonnet',
    modelLabel: 'Sonnet 5.5',
    ...partial,
  }
}

function msg(id, role, text, extra) {
  return { id, role, text, ...extra }
}

export function seedWorld() {
  const chats = [
    chat({
      id: 'auth-race',
      title: 'fix auth token refresh race',
      harness: 'cc',
      model: 'sonnet',
      modelLabel: 'Sonnet 5.5',
      machine: 'mac-mini',
      status: 'running',
      project: 'webapp',
      contextUsed: 68000,
      messages: [
        msg('ar-u', 'user', 'the refresh-token call in gateway/auth.ts races the logout call and sometimes resurrects a dead session'),
        msg('ar-a', 'agent', 'Refresh and logout both write the session map in gateway/auth.ts. Logout deletes the row. A refresh that started earlier puts it back.'),
        msg('ar-t', 'tool', '', {
          name: 'read_file',
          arg: 'gateway/auth.ts',
          state: 'ok',
          result: 'refreshSession writes the row before logout’s delete is visible. Nothing locks the map.',
        }),
      ],
    }),
    chat({
      id: 'dark-mode',
      title: 'apply dark mode to settings screens',
      harness: 'pi',
      model: 'pi-1',
      modelLabel: 'Pi 1',
      machine: 'mac-mini',
      status: 'idle',
      project: 'webapp',
      contextUsed: 22000,
      messages: [
        msg('dm-u', 'user', 'apply dark mode to the settings screens'),
        msg('dm-a', 'agent', 'Settings now uses the same tokens as the shell. Those screens no longer ship a second palette.'),
      ],
    }),
    chat({
      id: 'cut-endpoints',
      title: 'cut unused endpoints from gateway',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'mac-mini',
      status: 'idle',
      project: 'webapp',
      contextUsed: 41000,
      messages: [
        msg('ce-u', 'user', 'cut unused endpoints from the gateway'),
        msg('ce-a', 'agent', 'Removed /v1/echo, /v1/preview, and /internal/warmup. The web app still calls the other twelve.'),
      ],
    }),
    chat({
      id: 'webp',
      title: 'migrate thumbnails to webp',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'server',
      status: 'idle',
      project: 'webapp',
      contextUsed: 15000,
      messages: [
        msg('wp-u', 'user', 'migrate thumbnails to webp'),
        msg('wp-a', 'agent', 'New uploads are written as webp. Existing png thumbnails stay until the file is replaced.'),
      ],
    }),
    chat({
      id: 'edge-502',
      title: 'trace 502s from the edge worker',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'server',
      status: 'running',
      project: 'api',
      contextUsed: 184000,
      compactQueued: true,
      messages: [
        msg('e5-u', 'user', 'trace the 502s from the edge worker'),
        msg('e5-a', 'agent', 'The worker logs an upstream reset just before the 502. Its timeout is 5s. The payments origin is slower than that when the webhook verifier runs.'),
      ],
    }),
    chat({
      id: 'build-hang',
      title: 'why does the build hang on windows',
      harness: 'cc',
      model: 'opus',
      modelLabel: 'Opus 5.5',
      machine: 'server',
      status: 'waiting',
      project: 'api',
      contextUsed: 54000,
      messages: [
        msg('bh-u', 'user', 'why does the build hang on windows'),
        msg('bh-a', 'agent', 'Compile finishes. It stops in the release link, with no output from link.exe. I need a verbose build to see which file it is waiting on.'),
        msg('bh-t', 'tool', '', {
          name: 'shell',
          arg: 'cargo build -vv --release',
          state: 'pending',
          onceResult: 'link.exe is blocked on a PDB lock held by a leftover mspdbsrv from the last run.',
          denyResult: 'Left the build alone. The hang is still in the link step.',
        }),
      ],
    }),
    chat({
      id: 'flaky-checkout',
      title: 'fix flaky checkout test',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'server',
      status: 'idle',
      project: 'api',
      contextUsed: 33000,
      messages: [
        msg('fc-u', 'user', 'the checkout test fails when the webhook is late'),
        msg('fc-t', 'tool', '', {
          name: 'shell',
          arg: 'npm test -- checkout',
          state: 'ok',
          result: 'Failed once: the assertion ran before the webhook. Passed after waiting for the order row.',
        }),
        msg('fc-a', 'agent', 'The test now waits for the webhook before it reads the order. The flake was the race, not the assertion.'),
      ],
    }),
    chat({
      id: 'payments-webhook',
      title: 'reroute the payments webhook',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'mac-mini',
      status: 'idle',
      project: 'payments',
      contextUsed: 12000,
      messages: [
        msg('pw-u', 'user', 'point the payments webhook at the payments service'),
        msg('pw-a', 'agent', 'The provider now posts to payments.example/hooks. The old gateway route returns 410.'),
      ],
    }),
    chat({
      id: 'env-vars',
      title: 'rename env vars before launch',
      harness: 'od',
      model: 'auto',
      modelLabel: 'Auto',
      machine: 'mac-mini',
      status: 'idle',
      project: 'payments',
      contextUsed: 47000,
      messages: [
        msg('ev-u', 'user', 'rename the env vars before launch'),
        msg('ev-a', 'agent', 'The launch names are PAYMENTS_SIGNING_KEY and PAYMENTS_WEBHOOK_SECRET. The old keys still work until the next deploy.'),
      ],
    }),
    chat({
      id: 'release-notes',
      title: 'release notes for 0.4.1',
      harness: 'cx',
      model: 'gpt6',
      modelLabel: 'GPT-6',
      machine: 'mac-mini',
      status: 'idle',
      project: 'site',
      contextUsed: 8000,
      messages: [
        msg('rn-u', 'user', 'draft the release notes for 0.4.1'),
        msg('rn-a', 'agent', 'Drafted from the commits since 0.4.0. The notes are in site/release/0.4.1.md and are not published.'),
      ],
    }),
  ]

  const devices = [
    {
      id: 'mac-mini',
      name: 'mac-mini',
      kind: 'desktop',
      platform: 'macOS',
      status: 'online',
      spec: 'macOS · M2 · 16 GB',
      since: '14 days',
      downSlices: [10],
      harnesses: ['cc', 'cx', 'pi', 'od'],
      agents: [
        { harness: 'cc', model: 'Sonnet 5.5', status: 'active', chatId: 'auth-race', activity: 'fix auth token refresh race' },
        { harness: 'cx', model: 'GPT-6', status: 'idle', chatId: 'release-notes', activity: 'release notes for 0.4.1' },
        { harness: 'pi', model: 'Pi 1', status: 'idle', chatId: 'dark-mode', activity: 'apply dark mode to settings screens' },
        { harness: 'od', model: 'Auto', status: 'idle', chatId: 'cut-endpoints', activity: 'cut unused endpoints from gateway' },
      ],
    },
    {
      id: 'server',
      name: 'server',
      kind: 'desktop',
      platform: 'Ubuntu',
      status: 'online',
      spec: 'Ubuntu · 24.04 · 32 GB',
      since: '31 days',
      downSlices: [30, 31],
      harnesses: ['od', 'cc'],
      agents: [
        { harness: 'od', model: 'Auto', status: 'active', chatId: 'edge-502', activity: 'trace 502s from the edge worker' },
        { harness: 'cc', model: 'Opus 5.5', status: 'idle', chatId: 'build-hang', activity: 'why does the build hang on windows' },
      ],
    },
    {
      id: 'thinkpad',
      name: 'thinkpad',
      kind: 'laptop',
      platform: 'Fedora',
      status: 'offline',
      spec: 'Fedora · 42 · 16 GB',
      lastSeen: '2h ago',
      downSlices: [44, 45, 46, 47],
      harnesses: ['cx'],
      agents: [
        { harness: 'cx', model: 'GPT-6', status: 'idle', chatId: null, activity: 'last session 3 days ago' },
      ],
    },
  ]

  return decorate({
    schema: 2,
    mode: 'local',
    reduceMotion: false,
    appVersion: '0.4.1',
    defaults: { harness: 'cc', model: 'sonnet', deviceId: 'mac-mini', project: 'webapp' },
    signedIn: ['cc', 'cx', 'od'],
    plugins: [
      installation('github', 'mac-mini'),
      installation('postgres', 'server'),
      installation('sentry', 'server', { hasKey: true, status: 'running' }),
    ],
    automations: [
      {
        id: 'ci-main',
        name: 'Main went red',
        when: 'The api pipeline on main fails',
        harness: 'cc',
        model: 'opus',
        modelLabel: 'Opus 5.5',
        deviceId: 'server',
        project: 'api',
        task: 'The release build on Windows is hanging in the link step. Find the lock and clear it.',
        trigger: { kind: 'pipeline', project: 'api', branch: 'main' },
        starterId: null,
        enabled: true,
        lastChatId: 'build-hang',
        lastAt: Date.parse('2026-09-30T16:40:00Z'),
      },
      {
        id: 'edge',
        name: 'Edge 502s',
        when: 'The edge worker returns 502 more than twice in five minutes',
        harness: 'od',
        model: 'auto',
        modelLabel: 'Auto',
        deviceId: 'server',
        project: 'api',
        task: 'Trace the 502s from the edge worker. The origin timeout is the suspect.',
        trigger: { kind: 'errors', project: 'api' },
        starterId: null,
        enabled: true,
        lastChatId: 'edge-502',
        lastAt: Date.parse('2026-09-30T18:05:00Z'),
      },
      {
        id: 'webhook',
        name: 'Webhook signature',
        when: 'Payments rejects a webhook signature',
        harness: 'od',
        model: 'auto',
        modelLabel: 'Auto',
        deviceId: 'mac-mini',
        project: 'payments',
        task: 'A payments webhook was rejected for a bad signature. Find which signer drifted.',
        trigger: { kind: 'webhook', project: 'payments' },
        starterId: null,
        enabled: false,
        lastChatId: null,
        lastAt: null,
      },
    ],
    chats,
    devices,
  })
}

export function loadWorld(raw) {
  if (!raw) return seedWorld()
  try {
    const parsed = JSON.parse(raw)
    if (!parsed || parsed.schema !== 2 || !Array.isArray(parsed.chats) || !Array.isArray(parsed.devices)) {
      return seedWorld()
    }
    if (Array.isArray(parsed.plugins)) parsed.plugins = parsed.plugins.filter((p) => p && p.deviceId)
    return decorate(parsed)
  } catch {
    return seedWorld()
  }
}

export function resetWorld() {
  return seedWorld()
}

function decorate(world) {
  const devices = world.devices.map((d) => {
    const agents = (d.agents ?? []).map((a) => {
      const linked = a.chatId && world.chats.find((c) => c.id === a.chatId)
      if (!linked) return a
      const active = d.status === 'online' && (linked.status === 'running' || linked.status === 'waiting')
      return {
        ...a,
        status: active ? 'active' : 'idle',
        ...(a.live
          ? { harness: linked.harness, model: linked.modelLabel, activity: linked.title }
          : {}),
      }
    })
    const platform = d.platform ?? 'Machine'
    const n = agents.length
    let detail = `${platform} · ${n} agent${n === 1 ? '' : 's'}`
    if (d.status === 'waiting') detail = 'Waiting for the agent'
    else if (d.status === 'offline') detail = `${platform} · offline`
    return { ...d, agents, detail }
  })
  return { ...world, devices }
}

function mapChat(world, chatId, fn) {
  return { ...world, chats: world.chats.map((c) => (c.id === chatId ? fn(c) : c)) }
}

function pendingTool(chat) {
  return chat?.messages?.some((m) => m.role === 'tool' && m.state === 'pending')
}

export function addDevice(world, { os, name }) {
  const hostname = slug(name)
  if (!hostname) return world
  if (world.devices.some((d) => d.id === hostname)) return world
  const meta = OS[os] ?? OS.linux
  const device = {
    id: hostname,
    name: hostname,
    kind: meta.kind,
    platform: meta.platform,
    status: 'waiting',
    spec: meta.spec,
    since: null,
    lastSeen: null,
    downSlices: [],
    harnesses: [],
    agents: [],
  }
  return decorate({ ...world, devices: [...world.devices, device] })
}

export function setDeviceOnline(world, id, online) {
  return decorate({
    ...world,
    devices: world.devices.map((d) => {
      if (d.id !== id || d.status === 'waiting') return d
      if (online) {
        return {
          ...d,
          status: 'online',
          lastSeen: undefined,
          downSlices: (d.downSlices ?? []).filter((s) => s !== 47),
        }
      }
      const down = new Set(d.downSlices ?? [])
      down.add(47)
      return { ...d, status: 'offline', lastSeen: 'just now', downSlices: [...down] }
    }),
  })
}

export function signInHarness(world, id) {
  if (!id || world.signedIn.includes(id)) return world
  return { ...world, signedIn: [...world.signedIn, id] }
}

export function signOutHarness(world, id) {
  return { ...world, signedIn: world.signedIn.filter((h) => h !== id) }
}

export function installOnDevice(world, deviceId, harnessId) {
  if (!world.signedIn.includes(harnessId)) return world
  return decorate({
    ...world,
    devices: world.devices.map((d) => {
      if (d.id !== deviceId || d.status !== 'online') return d
      if ((d.harnesses ?? []).includes(harnessId)) return d
      return { ...d, harnesses: [...d.harnesses, harnessId] }
    }),
  })
}

export function setMode(world, mode) {
  return { ...world, mode: mode === 'cloud' ? 'cloud' : 'local' }
}

export function setReduceMotion(world, value) {
  return { ...world, reduceMotion: !!value }
}

export function setDefaults(world, patch) {
  return { ...world, defaults: { ...world.defaults, ...patch } }
}

export function markUpdated(world, version) {
  return { ...world, appVersion: version }
}

export function setPlugin(world, id, patch) {
  return {
    ...world,
    plugins: world.plugins.map((p) => (p.id === id ? { ...p, ...patch } : p)),
  }
}

function pluginStatus(plugin, running) {
  if (!running) return 'off'
  return plugin.needsKey && !plugin.hasKey ? 'needs-key' : 'running'
}

function installation(catalogId, deviceId, extra) {
  const entry = PLUGINS.find((p) => p.id === catalogId)
  const needsKey = !!entry.needsKey
  return {
    id: `${catalogId}@${deviceId}`,
    catalogId,
    name: entry.name,
    detail: entry.detail,
    command: entry.command,
    deviceId,
    needsKey,
    hasKey: false,
    status: needsKey ? 'needs-key' : 'running',
    ...extra,
  }
}

export function installFromCatalog(world, catalogId, deviceId) {
  const entry = PLUGINS.find((p) => p.id === catalogId)
  const device = world.devices.find((d) => d.id === deviceId)
  if (!entry || !device || device.status !== 'online') return world
  const id = `${catalogId}@${deviceId}`
  if (world.plugins.some((p) => p.id === id)) return world
  return { ...world, plugins: [...world.plugins, installation(catalogId, deviceId)] }
}

export function addCustomPlugin(world, input) {
  const name = String(input?.name ?? '').trim()
  const detail = String(input?.detail ?? '').trim()
  const command = String(input?.command ?? '').trim()
  const device = world.devices.find((d) => d.id === input?.deviceId)
  if (!name || !detail || !command || !device || device.status !== 'online') return world
  const taken = world.plugins.some((p) => p.deviceId === device.id && p.name.toLowerCase() === name.toLowerCase())
  if (taken) return world
  const needsKey = !!input.needsKey
  return {
    ...world,
    plugins: [...world.plugins, {
      id: nid('plugin'),
      catalogId: null,
      name,
      detail,
      command,
      deviceId: device.id,
      needsKey,
      hasKey: false,
      status: needsKey ? 'needs-key' : 'running',
    }],
  }
}

export function removePlugin(world, id) {
  return { ...world, plugins: world.plugins.filter((p) => p.id !== id) }
}

export function acknowledgePluginKey(world, id) {
  const plugin = world.plugins.find((p) => p.id === id)
  if (!plugin || !plugin.deviceId) return world
  return setPlugin(world, id, { hasKey: true, status: 'running' })
}

export function setPluginEnabled(world, id, enabled) {
  const plugin = world.plugins.find((p) => p.id === id)
  if (!plugin) return world
  if (enabled && !plugin.deviceId) return world
  return setPlugin(world, id, { status: pluginStatus(plugin, enabled) })
}

export function toggleAutomation(world, id) {
  return {
    ...world,
    automations: world.automations.map((a) => (a.id === id ? { ...a, enabled: !a.enabled } : a)),
  }
}

function ruleFrom(input, id) {
  const name = String(input?.name ?? '').trim()
  const task = String(input?.task ?? '').trim()
  if (!name || !task) return null
  const trigger = normalizeTrigger(input.trigger)
  return {
    id,
    name,
    when: whenSentence(trigger),
    trigger,
    harness: input.harness,
    model: input.model,
    modelLabel: input.modelLabel,
    deviceId: input.deviceId,
    project: input.project || trigger.project,
    task,
    enabled: input.enabled !== false,
    starterId: input.starterId ?? null,
  }
}

export function addAutomation(world, input) {
  const rule = ruleFrom(input, input?.id ?? nid('rule'))
  if (!rule) return world
  return { ...world, automations: [{ ...rule, lastChatId: null, lastAt: null }, ...world.automations] }
}

export function addStarter(world, starterId) {
  const starter = AUTOMATION_STARTERS.find((s) => s.id === starterId)
  if (!starter) return world
  if (world.automations.some((a) => a.starterId === starterId)) return world
  return addAutomation(world, {
    name: starter.name,
    trigger: starter.trigger,
    harness: starter.harness,
    model: starter.model,
    modelLabel: starter.modelLabel,
    deviceId: starter.deviceId,
    project: starter.project,
    task: starter.task,
    starterId: starter.id,
    enabled: true,
  })
}

export function updateAutomation(world, id, input) {
  const existing = world.automations.find((a) => a.id === id)
  if (!existing) return world
  const rule = ruleFrom({ ...input, starterId: existing.starterId }, id)
  if (!rule) return world
  return {
    ...world,
    automations: world.automations.map((a) => (a.id === id ? { ...a, ...rule, lastChatId: a.lastChatId, lastAt: a.lastAt } : a)),
  }
}

export function removeAutomation(world, id) {
  return { ...world, automations: world.automations.filter((a) => a.id !== id) }
}

export function taskPlacement(world, input) {
  const name = input.harnessName ?? HARNESS_NAMES[input.harness] ?? input.harness
  if (!world.signedIn.includes(input.harness)) {
    return { status: 'idle', note: `${name} isn't signed in. Sign it in from Settings.` }
  }
  if (input.mode !== 'cloud') return { status: 'running', note: null }
  const device = world.devices.find((d) => d.id === input.deviceId)
  if (!device || device.status === 'waiting') {
    return { status: 'idle', note: `${device?.name ?? 'That machine'} has not checked in yet.` }
  }
  if (device.status === 'offline') {
    return { status: 'idle', note: `${device.name} is offline. The task stays here until it reconnects.` }
  }
  if (!(device.harnesses ?? []).includes(input.harness)) {
    return { status: 'idle', note: `${name} is signed in, but it is not installed on ${device.name}.` }
  }
  return { status: 'running', note: null }
}

export function openChat(world, input) {
  const id = input.id ?? nid('chat')
  const cloud = input.mode === 'cloud' && input.deviceId
  const place = taskPlacement(world, input)
  const messages = [msg(nid('user'), 'user', String(input.text ?? '').trim())]
  if (place.note) messages.push(msg(nid('note'), 'note', place.note))
  const created = {
    id,
    title: titleFrom(input.text),
    harness: input.harness,
    model: input.model,
    modelLabel: input.modelLabel,
    machine: cloud ? input.deviceId : null,
    status: place.status,
    project: input.project ?? 'webapp',
    messages,
    contextUsed: usageFor(input.text),
    compactQueued: false,
    approvals: [],
  }
  let devices = world.devices
  if (cloud && place.status === 'running') {
    devices = devices.map((d) => {
      if (d.id !== input.deviceId) return d
      return {
        ...d,
        agents: [
          ...d.agents,
          {
            harness: created.harness,
            model: created.modelLabel,
            status: 'active',
            chatId: id,
            activity: created.title,
            live: true,
          },
        ],
      }
    })
  }
  return decorate({ ...world, devices, chats: [created, ...world.chats] })
}

export function runAutomation(world, id, now = Date.now()) {
  const rule = world.automations.find((a) => a.id === id)
  if (!rule || !rule.enabled) return { world, chatId: null }
  const chatId = nid('chat')
  let next = openChat(world, {
    id: chatId,
    text: rule.task,
    harness: rule.harness,
    model: rule.model,
    modelLabel: rule.modelLabel,
    deviceId: rule.deviceId,
    mode: 'cloud',
    project: rule.project,
  })
  next = {
    ...next,
    automations: next.automations.map((a) => (a.id === id ? { ...a, lastChatId: chatId, lastAt: now } : a)),
  }
  return { world: next, chatId }
}

export function sendMessage(world, chatId, text) {
  const body = String(text ?? '').trim()
  if (!body) return world
  const chat = world.chats.find((c) => c.id === chatId)
  if (!chat || pendingTool(chat)) return world
  const window = contextWindow(chat.model)
  let used = chat.contextUsed ?? 0
  const extra = []
  if (chat.compactQueued) {
    used = Math.round(0.4 * window)
    extra.push(msg(nid('note'), 'note', 'This session is past 92% of its window. The transcript was compacted before this message.'))
  }
  used += usageFor(body)
  return decorate(mapChat(world, chatId, (c) => ({
    ...c,
    status: 'running',
    contextUsed: used,
    compactQueued: used / window >= 0.92,
    messages: [...c.messages, ...extra, msg(nid('user'), 'user', body)],
  })))
}

export function resolveTool(world, chatId, messageId, choice) {
  const chat = world.chats.find((c) => c.id === chatId)
  if (!chat) return world
  const tool = chat.messages.find((m) => m.id === messageId && m.role === 'tool')
  if (!tool || tool.state !== 'pending') return world
  const allow = choice === 'once' || choice === 'session'
  const state = allow ? 'ok' : 'denied'
  const result = allow ? tool.onceResult : tool.denyResult
  const approvals = choice === 'session' ? [...(chat.approvals ?? []), tool.name] : chat.approvals ?? []
  const messages = chat.messages.map((m) => (m.id === messageId ? { ...m, state, result } : m))
  return decorate(mapChat(world, chatId, (c) => ({
    ...c,
    status: 'idle',
    approvals,
    messages,
  })))
}

export function setChatHarness(world, chatId, harnessId, modelId, modelLabel) {
  return decorate(mapChat(world, chatId, (c) => ({
    ...c,
    harness: harnessId,
    model: modelId,
    modelLabel,
  })))
}

export function setChatModel(world, chatId, modelId, modelLabel) {
  return decorate(mapChat(world, chatId, (c) => ({ ...c, model: modelId, modelLabel })))
}
