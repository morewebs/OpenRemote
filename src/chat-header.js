import { displayWord } from './status-words.js'

export const STATUS_LABEL = {
  starting: 'Starting',
  running: 'Running',
  waiting: 'Needs your decision',
  stopped: 'Stopped',
  failed: 'Failed',
}

export function statusLine(status) {
  const state = displayWord(status)
  const label = STATUS_LABEL[state]
  return label ? { kind: 'status', state, label } : null
}

export function folderName(path) {
  if (typeof path !== 'string' || !path) return 'Home folder'
  if (/^(?:[a-z]:)?[\\/]+$/i.test(path)) return path
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path
}

export function metaItems(chat, { app, device = null, gitPlace = null }) {
  return [
    { kind: 'folder', label: folderName(chat.workspace), title: chat.workspace ?? undefined },
    ...(gitPlace ? [{ kind: 'branch', label: gitPlace.branch, title: gitPlace.title }] : []),
    { kind: 'app', label: app, device },
    statusLine(chat.status),
  ].filter(Boolean)
}

export function chatRights(chat, { access, web, cloudReady, canRename, canManageAccount } = {}) {
  const reachable = access === 'here' || access === 'remote'
  return {
    rename: Boolean(canRename),
    account: reachable && Boolean(canManageAccount && chat.appAccountId),
    sync: !web && !chat.executor && Boolean(cloudReady),
    // Only synced chats have a delete operation; private chats can be archived.
    delete: !web && Boolean(chat.executor),
    machine: !web && access === 'not-machine',
  }
}

export function menuItems(rights, { copyable, accountDisconnected } = {}) {
  const groups = [
    [
      rights.rename && { id: 'rename', label: 'Rename' },
      { id: 'copy', label: 'Copy as Markdown', disabled: !copyable },
      { id: 'details', label: 'Details' },
      rights.account && { id: 'account', label: accountDisconnected ? 'Reconnect App account' : 'Manage App account' },
    ],
    [
      rights.sync && { id: 'sync', label: 'Sync to Cloud' },
      rights.delete && { id: 'delete', label: 'Delete chat', danger: true },
    ],
  ].map((group) => group.filter(Boolean)).filter((group) => group.length)
  return groups.flatMap((group, i) => i ? [{ divider: true }, ...group] : group)
}

export function notices(chat, rights, { confirmation, error } = {}) {
  return [
    error && { kind: 'error', text: error },
    chat.status === 'failed' && chat.lastError && { kind: 'failure', text: `This chat stopped with an error: ${chat.lastError}` },
    rights.machine && { kind: 'machine', text: 'This device is not set up to run chats for your other devices.' },
    confirmation === 'sync' && rights.sync && {
      kind: 'sync', text: 'Copy this chat, tool output included, to your other devices?', action: 'Sync', cancel: 'Cancel',
    },
    confirmation === 'delete' && rights.delete && {
      kind: 'delete', text: 'Delete this chat on all your devices?', action: 'Delete everywhere', cancel: 'Keep chat',
    },
  ].filter(Boolean)
}
