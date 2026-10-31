// What the browser's device view shows, as pure rules.
//
// A browser has no local daemon and no copy of the transcript, so everything
// here comes from the two routes a peer may ask a machine for: the synced-chat
// summary and a range of one synced chat's log. The shapes are the daemon's
// own (`crate::sync::summary` and `crate::sync::range`), and nothing here reads
// a field those routes do not send.

/// One summary row, or null for a payload that is not a summary row.
export function summaryRow(session, titles = {}) {
  if (!session || typeof session !== 'object') return null
  if (typeof session.id !== 'string' || !session.id) return null
  return {
    id: session.id,
    executor: typeof session.executor === 'string' ? session.executor : '',
    head: Number.isFinite(session.head) ? session.head : 0,
    updatedAt: Number.isFinite(session.updated_at) ? session.updated_at : 0,
    // The summary carries no title: a chat's name is in its log (a rename
    // event, or its first message), so it appears once the chat is opened.
    title: typeof titles[session.id] === 'string' ? titles[session.id] : null,
  }
}

/// The chats the machine says are synced, newest activity first. Tombstoned
/// chats are deleted and never listed.
export function summaryList(payload, titles = {}) {
  const buried = new Set(
    (Array.isArray(payload?.tombstones) ? payload.tombstones : [])
      .map((stone) => stone?.id)
      .filter((id) => typeof id === 'string'),
  )
  const rows = Array.isArray(payload?.sessions) ? payload.sessions : []
  return rows
    .map((session) => summaryRow(session, titles))
    .filter((row) => row && !buried.has(row.id))
    .sort((a, b) => b.updatedAt - a.updatedAt)
}

/// The line under a row's name, before the chat is opened: what the summary
/// can say, which is that it is synced and when it last changed. The executor
/// it names is a device id, and this page is already showing that machine, so
/// the id itself is noise here.
export function rowLine(row, formats = {}) {
  if (!row) return ''
  const when = formats.when ? formats.when(row.updatedAt) : ''
  const parts = ['synced from this computer']
  if (when) parts.push(`last changed ${when}`)
  return parts.join(' · ')
}

/// What the empty list says. The second sentence is the part people need:
/// syncing is the desktop app's command, not something this page can do.
export const EMPTY_SUMMARY =
  'No chats are synced from this computer yet. Open a chat in the desktop app and run its Sync command to bring it here.'

/// The cursor after one page: the highest seq actually folded, never a head
/// that the page did not carry. A range page stops at a byte budget, so a head
/// beyond the events is more to fetch, not a position to jump to. Events that
/// skip numbers (a gap) still move the cursor past them, or every poll would
/// ask for the same gap again.
export function advanceCursor(cursor, page) {
  let next = Number.isFinite(cursor) ? cursor : 0
  const events = Array.isArray(page?.events) ? page.events : []
  for (const event of events) {
    if (Number.isFinite(event?.seq) && event.seq > next) next = event.seq
  }
  return next
}

/// Whether the page brought anything at all, so the view can say "catching up"
/// only when there is something to catch.
export function pageSize(page) {
  return Array.isArray(page?.events) ? page.events.length : 0
}

/// The machine's own count of what its copy holds, for the status line.
export function headOf(page) {
  return Number.isFinite(page?.head) ? page.head : null
}

/// What a refused chat request means, in the words the view shows. 410 is the
/// daemon's tombstone: the chat was deleted, here or on another device, and
/// nothing will bring it back. 404 is a chat that is private (a private chat is
/// never handed out) or gone from this machine's keeping.
export function chatRefusal(err) {
  const status = err?.status
  if (status === 410) {
    return { sentence: 'This chat was deleted. It no longer exists on the computer that ran it.', stop: true }
  }
  if (status === 404) {
    return { sentence: 'This chat is private, or the computer no longer keeps it.', stop: true }
  }
  return { sentence: 'The computer did not answer. This page keeps trying while the chat is open.', stop: false }
}

/// The request that sends a prompt. The daemon's receipt contract needs the
/// request id, and the console already makes one per action.
export function promptRequest({ id, text, requestId }) {
  return {
    method: 'POST',
    path: `/sessions/${encodeURIComponent(id)}/prompts`,
    body: { request_id: requestId, text },
  }
}

/// The choices a decision offers: the harness's own options, each with the wire
/// id that is posted and the label the person reads.
export function decisionOptions(item) {
  const options = Array.isArray(item?.options) ? item.options : []
  return options.filter((option) => option && (typeof option.id === 'string' || typeof option.id === 'number'))
}

/// The label of the choice a decision was answered with, the way the desktop
/// console names it: the option's own label, never its wire id.
export function answerLabel(item) {
  if (!item) return ''
  const found = decisionOptions(item).find((option) => option.id === item.answeredChoice)
  return found?.label ?? item.answeredChoice ?? ''
}

/// The request that answers a decision, with the same body keys the desktop
/// console sends (`request_id` and `choice`) and no others.
export function answerRequest({ decisionId, choice, requestId }) {
  return {
    method: 'POST',
    path: `/decisions/${encodeURIComponent(decisionId)}/answer`,
    body: { request_id: requestId, choice },
  }
}

/// Whether a decision row can send an answer now. One answer per decision,
/// one request at a time: a pending row that is already answered, already
/// sending, or carrying no choice is not answerable.
export function canAnswerDecision({ pending, answeredChoice, answering, choice } = {}) {
  return Boolean(pending) && answeredChoice == null && !answering && choice != null
}

/// What a refused answer says. The daemon's own sentence when it sent one
/// ("no such decision" is more use than a generic failure), and a plain
/// sentence otherwise; the row stays answerable either way.
export function answerRefusal(err) {
  const said = typeof err?.message === 'string' ? err.message.trim() : ''
  return said || 'The computer refused the answer.'
}

/// Whether the composer has anything worth sending.
export function canSend(text) {
  return typeof text === 'string' && text.trim().length > 0
}
