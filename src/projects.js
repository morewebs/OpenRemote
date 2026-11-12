// Projects: the pure helpers every surface shares. A project is its
// folders - the name is the folder's basename, never stored, so the disk
// stays the only place it lives.

import { workspaceName } from './state/reducer.js'
import { isWaiting } from './needs-you.js'
import { canonical } from './status-words.js'

/** A project's name: its first folder's basename. */
export function projectName(project) {
  const folder = project?.folders?.[0]
  return folder ? workspaceName(folder) : 'project'
}

/**
 * A project with no folder behind it: the registration survived, the folder it named does
 * not. The sidebar draws it as its own case rather than as a live project called
 * "project", because there is nothing there to unregister and its chats have already
 * fallen back to the flat list.
 */
export function folderless(project) {
  return !(project?.folders ?? []).length
}

/**
 * The names two or more projects share. A name is a folder's basename, so `/work/app` and
 * `/home/app` both read "app": without the folder, a head, its Remove row and its New-chat
 * label are indistinguishable.
 */
export function duplicatedNames(projects) {
  const counts = new Map()
  for (const project of projects ?? []) {
    const name = projectName(project)
    counts.set(name, (counts.get(name) ?? 0) + 1)
  }
  return new Set([...counts.entries()].filter(([, count]) => count > 1).map(([name]) => name))
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
 * The sidebar's sections: registered projects (each with the chats that ran
 * in its folders), then the flat chats - every chat that belongs to no
 * project, in one list.
 *
 * Every registered project comes back, including one with no chats: the first
 * thing a new user does is register a folder, and a project that is missing
 * from the list until a chat exists in it reads as an action the app dropped.
 * The caller draws the zero and says the project has no chats yet.
 */
/// Where each project's folders point, built once per `projects` change.
/// The sidebar groups on every beat, and a scan of every project per row made
/// that cost chats x projects x folders: the wrong way round for a list that
/// grows. First project wins, the same rule `projectFor` has.
export function projectIndex(projects) {
  const byFolder = new Map()
  for (const project of projects ?? []) {
    for (const folder of project.folders ?? []) {
      if (!byFolder.has(folder)) byFolder.set(folder, project)
    }
  }
  return byFolder
}

/// The projects that hold an unread chat, in one pass over `rows`. Counting
/// after grouping re-walked every group, which is the same quadratic in a
/// different place.
export function unreadProjects(rows, unread, index) {
  const hot = new Set()
  if (!unread || unread.size === 0) return hot
  for (const row of rows ?? []) {
    if (!unread.has(row.id)) continue
    const project = index?.get(row.workspace) ?? null
    if (project) hot.add(project.id)
  }
  return hot
}

export function sections(projects, rows, index = null) {
  const byProject = new Map((projects ?? []).map((p) => [p.id, { project: p, items: [] }]))
  const byFolder = index ?? projectIndex(projects)
  const flat = []
  for (const row of rows ?? []) {
    const project = byFolder.get(row.workspace) ?? null
    if (project && byProject.has(project.id)) byProject.get(project.id).items.push(row)
    else flat.push(row)
  }
  return {
    projects: [...byProject.values()],
    flat,
  }
}

/**
 * How many of a project's chats are waiting, in one pass over the waiting rows.
 *
 * The sidebar draws those chats in the "Needs you" group above the projects,
 * not under their project, so the grouping never sees them. Counting here,
 * against the same project index, keeps the two surfaces about the same chats:
 * the tally a head shows comes from the very rows the group is drawn from, and
 * a chat cannot be counted twice (it is in `rows` once, and in one project).
 *
 * The predicate is the row's own waiting rule (needs-you.js), so a head cannot
 * call a chat waiting while its row shows busy or idle: a live status wins
 * over the polled one, exactly as the amber dot does.
 */
export function waitingProjects(rows, index) {
  const counts = new Map()
  for (const row of rows ?? []) {
    if (!row || !isWaiting(row.liveStatus, row.polledStatus)) continue
    const project = index?.get(row.workspace) ?? null
    if (!project) continue
    counts.set(project.id, (counts.get(project.id) ?? 0) + 1)
  }
  return counts
}

/**
 * What a project's head says beside its name, or null when there is nothing to
 * say: a list of `{ kind, text }` parts, in the order a person reads them.
 *
 * Two facts are worth the space, and each has its own reason. How many chats
 * the project holds answers the question a *collapsed* head raises - is there
 * anything in here - so it appears only while the project is collapsed; the
 * expanded list is its own answer, which is why a project with chats and
 * nothing waiting shows just its name. How many are waiting is not visible
 * from the list without reading every row's status, so it is said in both
 * states, and it is the same rule the row's amber dot uses (`isWaiting`),
 * never a count of a second kind.
 *
 * The parts are kept apart rather than glued into one string so the head can
 * colour the waiting part with the status colour and leave the rest quiet:
 * a count in the same amber as a waiting row reads as a warning, not a tally.
 */
export function projectLine({ chats = 0, waiting = 0, failed = 0, running = 0 } = {}, { collapsed = false } = {}) {
  const parts = []
  if (collapsed) {
    parts.push({ kind: 'chats', text: `${chats} ${chats === 1 ? 'chat' : 'chats'}` })
  }
  if (waiting > 0) {
    parts.push({ kind: 'waiting', text: `${waiting} waiting` })
  }
  // Folded, the rows that would show a failure or a running turn are out of
  // sight, so the head rolls them up (ported from br-sidebar 2a3a990, in
  // main's words rather than the branch's row dots). Open, the rows say it.
  if (collapsed && failed > 0) parts.push({ kind: 'failed', text: `${failed} failed` })
  if (collapsed && running > 0) parts.push({ kind: 'running', text: `${running} running` })
  return parts.length ? parts : null
}

/**
 * How many of `rows` failed and how many are running (starting or working),
 * for a folded project's head. Either status vocabulary counts the same. A
 * copy whose device is away is skipped: its last status is stale. Waiting
 * chats are not counted here - they sit in the group above, and the head
 * counts them from there (`waitingProjects`).
 */
export function heldStates(rows) {
  let failed = 0
  let running = 0
  for (const row of rows ?? []) {
    if (!row || row.offline) continue
    const word = canonical(row.status)
    if (word === 'failed') failed += 1
    else if (word === 'starting' || word === 'working') running += 1
  }
  return { failed, running }
}
