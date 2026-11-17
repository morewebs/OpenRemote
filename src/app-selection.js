import { pickable } from './harness-manager.js'

/** App-specific model, effort and permission words never cross App boundaries. */
export function appSelection(chat, app, { projects = [], thisDevice = null } = {}) {
  if (!chat || !app || !pickable(app) || chat.harness === app.id) return null
  const target = chat.executor && chat.executor !== thisDevice ? chat.executor : null
  const seed = { harness: app.id, target, workspace: chat.workspace ?? null }
  if (!target) {
    const project = projects.find(p => p.folders?.includes(chat.workspace))
    if (project) seed.projectId = project.id
  }
  return seed
}

export function carryAppDraft(waiting, incoming) {
  if (!incoming || waiting === incoming) return waiting
  return waiting ? `${waiting}\n\n${incoming}` : incoming
}
