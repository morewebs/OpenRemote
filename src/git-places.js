// Where each chat's folder stands in git: its branch, and whether it is a
// linked worktree of another folder's repository (Worktrunk's, or any
// `git worktree add`). The daemon reads it from git's own files
// (POST /git/places); this keeps one small shared cache, asks for many
// folders in one request, and reads again every little while so a branch
// switched in a terminal shows up.
//
// Only folders on this computer: a chat running on another device has its
// folder there, and this device can't read it.

import { useEffect, useSyncExternalStore } from 'react'

export const REFRESH_MS = 15_000

// What a chat's header and its sidebar row say about a place.
export function placeWords(place) {
  if (!place) return null
  const branch = place.branch ?? (place.detached ? `detached at ${place.detached}` : null)
  if (!branch) return null
  const mainName = String(place.main ?? '').split(/[\\/]/).filter(Boolean).pop() ?? ''
  return {
    branch,
    worktree: Boolean(place.worktree),
    // The folder's own repository, for a linked worktree.
    of: place.worktree ? mainName : null,
    title: place.worktree
      ? `Branch ${branch}, in a worktree of ${place.main}`
      : `Branch ${branch}`,
  }
}

// The cache, shared by every view that asks.
export function placeStore() {
  const places = new Map()
  const listeners = new Set()
  let version = 0
  let pending = new Set()
  let flushing = null
  const store = {
    get: (path) => places.get(path) ?? null,
    version: () => version,
    subscribe(listener) {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    // Ask for these folders; requests made together go as one.
    ask(api, paths) {
      for (const p of paths) if (typeof p === 'string' && p) pending.add(p)
      if (flushing || !pending.size || !api?.gitPlaces) return flushing
      flushing = Promise.resolve().then(async () => {
        const asked = [...pending].slice(0, 200)
        pending = new Set([...pending].slice(200))
        try {
          const reply = await api.gitPlaces(asked)
          const found = reply?.places ?? {}
          let changed = false
          for (const p of asked) {
            const next = found[p] ?? null
            if (JSON.stringify(places.get(p) ?? null) !== JSON.stringify(next)) {
              places.set(p, next)
              changed = true
            }
          }
          if (changed) {
            version++
            for (const l of listeners) l()
          }
        } catch {
          /* an older daemon without the route, or offline: no branch shown */
        } finally {
          flushing = null
          // Folders asked for while this request was out go next.
          if (pending.size) store.ask(api, [])
        }
      })
      return flushing
    },
  }
  return store
}

export const gitPlaces = placeStore()

// The places of `paths` (folders on this computer), kept fresh while the
// caller is on screen. Answers a function from a path to its place.
export function useGitPlaces(api, paths) {
  const key = [...new Set((paths ?? []).filter(Boolean))].sort().join('\n')
  useSyncExternalStore(gitPlaces.subscribe, gitPlaces.version)
  useEffect(() => {
    if (!key || !api) return undefined
    const list = key.split('\n')
    gitPlaces.ask(api, list)
    const timer = setInterval(() => {
      if (typeof document === 'undefined' || document.visibilityState !== 'hidden') gitPlaces.ask(api, list)
    }, REFRESH_MS)
    return () => clearInterval(timer)
  }, [api, key])
  return (path) => gitPlaces.get(path)
}
