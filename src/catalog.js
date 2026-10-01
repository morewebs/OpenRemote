import {
  Asterisk,
  Code,
  DesktopTower,
  Laptop,
  Lightning,
  OpenAiLogo,
  Pi,
  Planet,
} from '@phosphor-icons/react'
import { siClaudecode, siCursor, siGooglegemini, siOpencode } from 'simple-icons'

// downSlices: indices of the 48 half-hour slices (last 24h, oldest
// left) that were down — drives the uptime band in the device modal
// app identity for the about popup — 0.4.1 matches the release-notes
// seed chat so the mock world stays coherent
export const APP = { version: '0.4.1', channel: 'dev', latest: '0.5.2' }

export const DEVICES = [
  {
    id: 'mac-mini',
    name: 'mac-mini',
    kind: 'desktop',
    status: 'online',
    detail: 'macOS · 4 agents',
    spec: 'macOS · M2 · 16 GB',
    since: '14 days',
    downSlices: [10],
    agents: [
      { harness: 'cc', model: 'Sonnet 5.5', status: 'active', chatId: 'auth-race', activity: 'fix auth token refresh race' },
      { harness: 'cx', model: 'GPT-6', status: 'active', chatId: 'release-notes', activity: 'release notes for 0.4.1' },
      { harness: 'pi', model: 'Pi 1', status: 'idle', chatId: 'dark-mode', activity: 'apply dark mode to settings screens' },
      { harness: 'od', model: 'Auto', status: 'idle', chatId: 'cut-endpoints', activity: 'cut unused endpoints from gateway' },
    ],
  },
  {
    id: 'server',
    name: 'server',
    kind: 'desktop',
    status: 'online',
    detail: 'Ubuntu · 2 agents',
    spec: 'Ubuntu · 24.04 · 32 GB',
    since: '31 days',
    downSlices: [30, 31],
    agents: [
      { harness: 'od', model: 'Auto', status: 'active', chatId: 'edge-502', activity: 'trace 502s from the edge worker' },
      { harness: 'cc', model: 'Opus 5.5', status: 'idle', chatId: 'build-hang', activity: 'why does the build hang on windows' },
    ],
  },
  {
    id: 'thinkpad',
    name: 'thinkpad',
    kind: 'laptop',
    status: 'offline',
    detail: 'Fedora · offline',
    spec: 'Fedora · 42 · 16 GB',
    lastSeen: '2h ago',
    downSlices: [44, 45, 46, 47],
    agents: [
      { harness: 'cx', model: 'GPT-6', status: 'idle', chatId: null, activity: 'last session 3 days ago' },
    ],
  },
]

export function deviceIcon(kind) {
  return kind === 'laptop' ? Laptop : DesktopTower
}

export const HARNESSES = [
  { id: 'cc', name: 'Claude Code', icon: Asterisk, brand: siClaudecode, group: 'Installed' },
  { id: 'cx', name: 'Codex', icon: OpenAiLogo, group: 'Installed' },
  { id: 'od', name: 'OpenCode', icon: Code, brand: siOpencode, group: 'Available' },
  { id: 'pi', name: 'Pi Agent', icon: Pi, group: 'Available' },
  { id: 'amp', name: 'Amp', icon: Lightning, group: 'Available' },
  { id: 'gem', name: 'Gemini CLI', icon: Asterisk, brand: siGooglegemini, group: 'Available' },
  { id: 'grok', name: 'Grok Build', icon: Planet, group: 'Available' },
  { id: 'cur', name: 'Cursor Agent', icon: Code, brand: siCursor, group: 'Available' },
]

export const MODELS = {
  cc: [
    { id: 'sonnet', name: 'Sonnet 5.5' },
    { id: 'opus', name: 'Opus 5.5' },
    { id: 'haiku', name: 'Haiku 4.5' },
    { id: 'sonnet-1m', name: 'Sonnet 5.5 1M' },
  ],
  cx: [
    { id: 'gpt6', name: 'GPT-6' },
    { id: 'gpt6-mini', name: 'GPT-6 mini' },
    { id: 'o5', name: 'o5' },
  ],
  od: [
    { id: 'auto', name: 'Auto' },
    { id: 'grok-code', name: 'Grok Code' },
    { id: 'kimi', name: 'Kimi K2' },
  ],
  pi: [{ id: 'pi-1', name: 'Pi 1' }],
  amp: [{ id: 'amp-1', name: 'Amp One' }],
  gem: [
    { id: 'gem-flash', name: 'Gemini Flash 3' },
    { id: 'gem-pro', name: 'Gemini Pro 3' },
  ],
  grok: [{ id: 'grok-4', name: 'Grok 4' }],
  cur: [
    { id: 'cur-auto', name: 'Auto' },
    { id: 'cur-frontier', name: 'Frontier' },
  ],
}

export function harnessName(id) {
  return HARNESSES.find((h) => h.id === id)?.name ?? id
}

export function modelName(harnessId, modelId) {
  const list = MODELS[harnessId] ?? []
  return list.find((m) => m.id === modelId)?.name ?? list[0]?.name ?? modelId
}

export { AUTOMATION_STARTERS, PLUGINS } from './library.js'
