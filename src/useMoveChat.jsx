import { useCallback, useState } from 'react'
import { Check, Folders, FolderSimple, FolderSimpleDashed } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { chatAccess, deviceName } from './cloud.js'
import { projectFor, projectName } from './projects.js'
import { askBeforeMove, moveTargets, unreachableNote } from './move-chat.js'
import { errorCopy } from './error-copy.js'
import MoveChatModal from './MoveChatModal.jsx'

/** The status line's words for a move that landed. */
export const movedLine = (to) => (to ? `Moved to ${projectName(to)}.` : 'Moved out of its project.')

/**
 * Moving chats between projects, for every surface that offers it: the
 * sidebar (its picker and its drop) and a pane's right-click menu. A chat's
 * projects are those of the machine that runs it: this computer's are at
 * hand, another machine's are asked for as its menu opens (`prepare`).
 * Every move asks first unless this device was told not to; the surface
 * renders `modal` once. `onNote` is the surface's status line: what a move
 * did, or the daemon's own words for why it didn't.
 *
 * A chat here is { id, title, workspace, executor }.
 */
export function useMoveChat({ onNote } = {}) {
  const { projects, apiFor, devices, thisDevice, cloud, moveChat } = useConsole()
  const [pending, setPending] = useState(null)
  // Another machine's projects, by device: { list } or { error }.
  const [remote, setRemote] = useState({})

  const accessOf = useCallback((chat) => chatAccess(chat, devices, thisDevice, cloud), [devices, thisDevice, cloud])

  // Where the chat's projects live: { list, device } when they can be
  // shown, { note, device } when they can't, and why.
  const placeOf = (chat) => {
    const access = accessOf(chat)
    if (access === 'here') return { list: projects ?? [] }
    const where = deviceName(devices, chat.executor)
    if (access === 'remote') {
      const known = remote[chat.executor]
      if (known?.list) return { list: known.list, device: where }
      return { note: known?.error ? `Couldn't reach ${where}` : 'Loading projects...', device: where }
    }
    return { note: unreachableNote(access, where), device: where }
  }

  // Ask another machine for its own projects as its chat's menu opens.
  const prepare = useCallback(
    (chat) => {
      if (accessOf(chat) !== 'remote') return
      const device = chat.executor
      apiFor(device)
        .projects()
        .then(
          (list) => setRemote((r) => ({ ...r, [device]: { list: Array.isArray(list) ? list : [] } })),
          (err) => setRemote((r) => ({ ...r, [device]: { error: errorCopy(err) } })),
        )
    },
    [accessOf, apiFor],
  )

  // Asked first unless this device was told not to; a move made without
  // asking says what it did, or the daemon's own words for why it didn't.
  const request = useCallback(
    (move) => {
      if (askBeforeMove()) {
        setPending({ move, failure: null })
        return
      }
      // A surface with no status line of its own (a pane's menu) hears a
      // failure from the dialog instead, which offers to try again.
      moveChat(move.chat.id, move.to?.id ?? null).then(
        () => onNote?.(movedLine(move.to)),
        (err) => (onNote ? onNote(errorCopy(err)) : setPending({ move, failure: errorCopy(err) })),
      )
    },
    [moveChat, onNote],
  )

  // Move `chat` to `to` (a project of its machine, or null for none).
  const moveTo = (chat, to, place = placeOf(chat)) =>
    request({
      chat,
      to: to ?? null,
      from: projectFor(place.list, chat.workspace),
      device: place.device ?? null,
    })

  // A menu's "Move to project" row: "No project" and each project of the
  // chat's machine, the current one checked and not offered again.
  const menuItem = (chat) => {
    const place = placeOf(chat)
    const submenu = place.list
      ? moveTargets(place.list, chat.workspace).map((target) => ({
          label: target.project ? <span className="pv">{target.name}</span> : target.name,
          icon: target.current ? (
            <Check size={14} />
          ) : target.project ? (
            <FolderSimple size={14} />
          ) : (
            <FolderSimpleDashed size={14} />
          ),
          disabled: target.current,
          onSelect: () => moveTo(chat, target.project, place),
        }))
      : [{ label: place.note, disabled: true }]
    return { label: 'Move to project', icon: <Folders size={14} />, submenu }
  }

  const closeModal = useCallback(() => setPending(null), [])
  const modal = pending && (
    <MoveChatModal
      move={pending.move}
      failure={pending.failure}
      onMoved={(move) => onNote?.(movedLine(move.to))}
      onClose={closeModal}
    />
  )

  return { prepare, placeOf, accessOf, menuItem, moveTo, modal }
}
