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
