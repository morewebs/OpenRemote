// Moving a chat between projects: the pure rules every surface shares - the
// sidebar's picker and its drop, the pane's menu, the confirm. A chat belongs
// to the project whose folder is its workspace, so a move changes that
// folder, and its agent works in the new one from its next message. The
// daemon decides whether a move happens; these only say what it would be.

import { projectFor, projectName } from './projects.js'
import { loadPrefs, savePrefs } from './settings.js'

/**
 * Where a chat can go: "No project" first, then the projects of the machine
 * that runs it. The one it is in now is marked current - a chat in a folder
 * no project holds is already in "No project".
 */
export function moveTargets(projects, workspace) {
  const here = projectFor(projects, workspace)
  return [
    { project: null, name: 'No project', current: !here },
    ...(projects ?? []).map((project) => ({
      project,
      name: projectName(project),
      current: here?.id === project.id,
    })),
  ]
}

/** Whether a drop on `project` (null: the Chats section) would move it. */
export function wouldMove(projects, workspace, project) {
  const here = projectFor(projects, workspace)
  return project ? here?.id !== project.id : here != null
}

/**
 * A running turn keeps a chat where it is. "Starting" doesn't: a harness
 * that names its conversation at the first message shows it until then, and
 * the daemon itself refuses the rare move that lands mid-spawn.
 */
export function busyReason(status) {
  if (status === 'running' || status === 'waiting') {
    return 'A turn is running in this chat. Wait for it to finish, or stop it, then move the chat.'
  }
  return null
}

/** The confirm's one sentence on what changes. */
export function moveSentence({ chatTitle, to }) {
  const where = to?.folders?.[0] ?? 'the home folder'
  return `From its next message the agent in "${chatTitle}" works in ${where}.`
}

/** Whether moves are asked first on this device. */
export function askBeforeMove() {
  return loadPrefs().ask_move_chat
}

/** The dialog's "Don't show this again", or Settings turning it back on. */
export function setAskBeforeMove(ask) {
  return savePrefs({ ask_move_chat: Boolean(ask) })
}

/**
 * Why a chat's projects can't be listed, for the chat's machine as
 * `chatAccess` (cloud.js) sees it; null when they can - here, or a
 * reachable machine that is asked for its own.
 */
export function unreachableNote(access, where) {
  switch (access) {
    case 'here':
    case 'remote':
      return null
    case 'offline':
      return `${where} is offline`
    case 'not-machine':
      return `${where} isn't a machine`
    case 'signed-out':
      return 'Sign in to Cloud first'
    case 'gone':
      return `${where} left your Cloud`
    default:
      return `Couldn't reach ${where}`
  }
}

/**
 * Whether dropping `row` (a sidebar chat row) on `project` (null: the Chats
 * section) would move it. The sidebar's projects are this computer's, so
 * only a chat that runs here moves by a drop.
 */
export function dropMoves(projects, row, project, access) {
  if (!row || access !== 'here') return false
  return wouldMove(projects, row.workspace, project)
}
