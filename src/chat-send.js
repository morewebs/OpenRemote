import { displayWord, isActive } from './status-words.js'

export function chatSendActions(chat, { reachable, hasDraft } = {}) {
  if (reachable) {
    if (isActive(chat.status)) return { runAction: 'interrupt', showSend: Boolean(hasDraft) }
    const status = displayWord(chat.status)
    // A chat stopped before it got a conversation has nothing to resume.
    if (chat.resumable && (status === 'stopped' || status === 'failed')) return { runAction: 'resume', showSend: false }
  }
  return { runAction: null, showSend: true }
}
