import { projectName } from './projects.js'

const KEY = 'openremote-project-arrangement'
const ids = value => [...new Set((Array.isArray(value) ? value : []).filter(id => typeof id === 'string' && id))]
const normalize = value => ({
  pins: ids(value?.pins),
  order: ids(value?.order),
  sort: ['manual', 'name', 'recent'].includes(value?.sort) ? value.sort : 'manual',
})

export function loadProjectPreferences(storage) {
  try { return normalize(JSON.parse((storage ?? globalThis.localStorage)?.getItem(KEY) ?? '{}')) } catch { return normalize(null) }
}

export function storeProjectPreferences(preferences, storage) {
  try { (storage ?? globalThis.localStorage)?.setItem(KEY, JSON.stringify(normalize(preferences))) } catch { /* Keep this run's arrangement. */ }
}

export function projectRows(project, rows) {
  const folders = new Set(project?.folders ?? [])
  return (rows ?? []).filter(row => folders.has(row.workspace))
}

export function collapseOtherProjects(projects, id) {
  return Object.fromEntries((projects ?? []).filter(project => project.id !== id).map(project => [project.id, true]))
}

export function pruneCollapsedProjects(collapsed, projects, answered) {
  if (!answered) return collapsed
  const known = new Set((projects ?? []).map(project => project.id))
  const entries = Object.entries(collapsed).filter(([id]) => known.has(id))
  return entries.length === Object.keys(collapsed).length ? collapsed : Object.fromEntries(entries)
}

export function orderProjects(projects, rows, preferences = {}) {
  const prefs = normalize(preferences)
  const pinned = new Set(prefs.pins)
  const rank = new Map(prefs.order.map((id, i) => [id, i]))
  const activity = new Map()
  const folders = new Map()
  for (const p of projects ?? []) for (const folder of p.folders ?? []) if (!folders.has(folder)) folders.set(folder, p.id)
  for (const row of rows ?? []) {
    const id = folders.get(row.workspace)
    if (id) activity.set(id, Math.max(activity.get(id) ?? 0, row.updatedAt ?? 0))
  }
  return [...(projects ?? [])].sort((a, b) => {
    const pin = Number(pinned.has(b.id)) - Number(pinned.has(a.id))
    if (pin) return pin
    if (prefs.sort === 'name') return projectName(a).localeCompare(projectName(b)) || a.id.localeCompare(b.id)
    if (prefs.sort === 'recent') {
      const recent = (activity.get(b.id) ?? 0) - (activity.get(a.id) ?? 0)
      if (recent) return recent
    }
    return (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity) || 0
  })
}

export function moveProject(displayed, id, delta, preferences = {}) {
  const list = [...displayed]
  const at = list.findIndex(p => p.id === id)
  const to = at + delta
  const pins = new Set(preferences.pins ?? [])
  if (at < 0 || to < 0 || to >= list.length || pins.has(id) !== pins.has(list[to].id)) return preferences
  ;[list[at], list[to]] = [list[to], list[at]]
  return { ...preferences, sort: 'manual', order: list.map(p => p.id) }
}

export function filterProjectRows(projects, rows, query = '', projectId = null) {
  const q = query.trim().toLowerCase()
  const scope = (projects ?? []).filter(p => !projectId || p.id === projectId)
  const folders = new Map()
  const matching = new Set()
  for (const p of scope) {
    if (q && [projectName(p), ...(p.folders ?? [])].some(s => s.toLowerCase().includes(q))) matching.add(p.id)
    for (const folder of p.folders ?? []) if (!folders.has(folder)) folders.set(folder, p.id)
  }
  const selected = (rows ?? []).filter(row => {
    const id = folders.get(row.workspace)
    if (projectId && id !== projectId) return false
    return !q || matching.has(id) || String(row.title ?? '').toLowerCase().includes(q)
  })
  const occupied = new Set(selected.map(row => folders.get(row.workspace)))
  return { projects: scope.filter(p => !q || matching.has(p.id) || occupied.has(p.id)), rows: selected }
}
