// Projects: the pure helpers every surface shares. A project is its
// folders - the name is the folder's basename, never stored, so the disk
// stays the only place it lives.

import { workspaceName } from './state/reducer.js'

/** A project's name: its first folder's basename. */
export function projectName(project) {
  const folder = project?.folders?.[0]
  return folder ? workspaceName(folder) : 'project'
}

/**
 * Group chats by project: a chat belongs to the project whose folders
 * contain its workspace. Chats with no workspace, or one no project
 * holds, belong to none - they render in the flat Chats section.
 */
export function projectFor(projects, workspace) {
  if (!workspace) return null
  return (projects ?? []).find((p) => (p.folders ?? []).includes(workspace)) ?? null
}

/**
 * The sidebar's two sections: registered projects (each with the chats
 * that ran in its folders), then the flat chats - every chat that
 * belongs to no project, in one list.
 */
export function sections(projects, rows) {
  const byProject = new Map((projects ?? []).map((p) => [p.id, { project: p, items: [] }]))
  const flat = []
  for (const row of rows ?? []) {
    const project = projectFor(projects, row.workspace)
    if (project) byProject.get(project.id).items.push(row)
    else flat.push(row)
  }
  return {
    projects: [...byProject.values()].filter((g) => g.items.length > 0),
    flat,
  }
}
