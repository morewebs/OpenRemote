// "Changes in this chat": every file a harness reported changing in the
// chat on screen, gathered from the transcript's own tool items.
//
// A chat draws a good split diff inside each tool card (diff.js) and then
// loses the shape of it across turns, so reviewing a long chat means
// scrolling and remembering. changes.js folds those same items by file;
// this modal is the list that fold produces, plus one file's diff at a
// time. It reads; it never stages, commits or reverts, and it says so,
// because the numbers come from what a harness put in a tool input rather
// than from the disk.
//
// It is keyed to a chat and opened through the request below, which the
// command palette uses. The shell mounts it once for the chat on screen
// (App.jsx), so nothing here needs a prop path through four components.

import { useEffect, useMemo, useRef, useState } from 'react'
import { ArrowLeft, X } from '@phosphor-icons/react'
import { fileChanges } from './changes.js'
import { harnessName } from './harness-names.js'
import { MAX_RENDER_ROWS, capRows } from './render-budget.js'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import './devices.css'
// Its own sheet, imported here rather than borrowed from the sidebar: the
// review belongs to the dialog, not to whatever happens to be mounted
// beside it.
import './changes.css'

// Anything that can reach the shell can ask for the review; the mounted
// modal answers. The same shape as popups.js, kept here because the thing
// it opens is this dialog and nothing else. A request carries the chat it
// is about, so a shell that later mounts one review per pane cannot open
// the wrong one: a request for another chat is dropped rather than shown
// under this chat's name.
const listeners = new Set()

/** Open the review for a chat; without an id, for the chat this modal holds. */
export function openChanges(chatId = null) {
  for (const fn of listeners) fn(chatId)
}

const OP_WORD = { added: 'added', edited: 'edited', deleted: 'deleted' }

function countWords(n) {
  return n === 1 ? '1 edit' : `${n} edits`
}

// The two counts a reviewer reads first: lines added, lines removed. Each
// carries its own colour so the pair reads at a glance, which is why the
// word travels with the class rather than being inferred from its position.
function lineWords(file) {
  const words = []
  if (file.addedLines > 0) words.push({ word: `+${file.addedLines}`, cls: 'chg-plus' })
  if (file.removedLines > 0) words.push({ word: `-${file.removedLines}`, cls: 'chg-minus' })
  return words
}

function statusWord(status) {
  if (status === 'failed') return 'last report failed'
  if (status === 'running') return 'in flight'
  return null
}

function DiffRows({ rows }) {
  const { rows: shown, hidden } = capRows(rows)
  if (rows.length === 0) {
    // A report can arrive with both sides identical; the card draws no row
    // for it, and an empty box would look like a fault.
    return <p className="chg-note">No lines differ in this report.</p>
  }
  return (
    <>
      <div className="chg-diff">
        {shown.map((row, i) => (
          <div className="chg-diff-row" key={i}>
            {/* A side that has nothing at this row keeps its cell on a wide
                window, so the two columns stay aligned; the class lets a
                phone drop it rather than paint a blank line. */}
            <span
              className={`chg-cell${row.left.kind === 'del' ? ' chg-cell--del' : ''}${
                row.left.text === '' ? ' chg-cell--none' : ''
              }`}
            >
              {row.left.text}
            </span>
            <span
              className={`chg-cell${row.right.kind === 'add' ? ' chg-cell--add' : ''}${
                row.right.text === '' ? ' chg-cell--none' : ''
              }`}
            >
              {row.right.text}
            </span>
          </div>
        ))}
      </div>
      {hidden > 0 && (
        <p className="chg-note">Showing the first {MAX_RENDER_ROWS} rows of {rows.length}.</p>
      )}
    </>
  )
}

export default function ChangesModal({ chat }) {
  const [open, setOpen] = useState(false)
  // The file whose diff is open, by the fold's own key, so a turn that keeps
  // editing it updates the open diff instead of closing it.
  const [selected, setSelected] = useState(null)
  const panelRef = useRef(null)

  useEffect(() => {
    const ask = (chatId) => {
      if (chatId && chat?.id !== chatId) return
      setSelected(null)
      setOpen(true)
    }
    listeners.add(ask)
    return () => {
      listeners.delete(ask)
    }
  }, [chat?.id])

  // The review belongs to one chat. The shell hands this modal whichever chat
  // the primary pane is on, so a navigation closes the review rather than
  // leaving one chat's files under another chat's name.
  useEffect(() => {
    setOpen(false)
    setSelected(null)
  }, [chat?.id])

  const close = () => {
    setOpen(false)
    setSelected(null)
  }
  // The family's shared dialog plumbing: focus in, Tab kept inside, the rest
  // of the app inert, focus back on the opener, and the Esc layer joined.
  // Android's back answers only while the review is on screen.
  useBack(open, close)
  useDialog(panelRef, close, { open })

  const folded = useMemo(() => fileChanges(chat?.timeline), [chat?.timeline])
  const file = selected ? folded.files.find((f) => f.key === selected) ?? null : null

  if (!open || !chat) return null

  const title = chat.title ?? 'this chat'
  const harness = harnessName(chat.harness) || 'the app'

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close()
      }}
    >
      <div
        ref={panelRef}
        className="dv-modal chg-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Changes in ${title}`}
        tabIndex={-1}
      >
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Changes in this chat</h2>
          <button className="dv-modal-close" onClick={close} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>

        <p className="dv-modal-hint">
          As <span className="pv">{title}</span> reported them through {harness}&apos;s own tools.
          Not a git diff: nothing here stages, commits or reverts.
        </p>

        {folded.files.length > 0 && !file && (
          <>
            <p className="chg-summary">
              {folded.totals.files} {folded.totals.files === 1 ? 'file' : 'files'}
              {' · '}
              {countWords(folded.totals.edits)}
              {folded.totals.addedLines > 0 && (
                <span className="chg-plus"> +{folded.totals.addedLines}</span>
              )}
              {folded.totals.removedLines > 0 && (
                <span className="chg-minus"> -{folded.totals.removedLines}</span>
              )}
              {/* The counts above are what landed. A report that did not land is said
                  here rather than folded into the headline, which a reviewer reads as
                  lines that reached the disk. */}
              {folded.unlandedEdits > 0 && (
                <span className="chg-unlanded">
                  {' '}
                  plus {countWords(folded.unlandedEdits)}
                  {folded.unlandedLines > 0 ? `, ${folded.unlandedLines} lines,` : ''} in a report
                  that failed
                </span>
              )}
            </p>
            <div className="chg-list">
              {folded.files.map((entry) => {
                const status = statusWord(entry.status)
                return (
                  <button
                    key={entry.key}
                    type="button"
                    className="chg-file"
                    onClick={() => setSelected(entry.key)}
                    title={entry.path}
                  >
                    <span className={`chg-op chg-op--${entry.op}`}>{OP_WORD[entry.op]}</span>
                    <span className="chg-path pv">{entry.path}</span>
                    <span className="chg-counts">
                      {lineWords(entry).map((count) => (
                        <span key={count.word} className={count.cls}>
                          {count.word}
                        </span>
                      ))}
                      <span className="chg-edits">{countWords(entry.edits)}</span>
                      {status && <span className="chg-status">{status}</span>}
                    </span>
                  </button>
                )
              })}
            </div>
          </>
        )}

        {file && (
          <>
            <div className="chg-detail-head">
              <button type="button" className="chg-back" onClick={() => setSelected(null)}>
                <ArrowLeft size={12} weight="bold" />
                All files
              </button>
              <span className={`chg-op chg-op--${file.op}`}>{OP_WORD[file.op]}</span>
              <span className="chg-path pv">{file.path}</span>
            </div>
            <p className="chg-summary">
              {countWords(file.edits)}
              {file.addedLines > 0 && <span className="chg-plus"> +{file.addedLines}</span>}
              {file.removedLines > 0 && <span className="chg-minus"> -{file.removedLines}</span>}
              {statusWord(file.status) && (
                <span className="chg-status"> {statusWord(file.status)}</span>
              )}
            </p>
            {file.parts.map((drawn, i) => (
              <div key={i} className="chg-part">
                {file.parts.length > 1 && (
                  <p className="chg-note">
                    Report {i + 1} of {file.parts.length}
                  </p>
                )}
                <DiffRows rows={drawn.rows} />
              </div>
            ))}
          </>
        )}

        {folded.empty === 'no-edit' && (
          <p className="chg-empty">
            {folded.unread > 0
              ? `No file changes this console could read. ${folded.unread === 1 ? 'One report named a file' : `${folded.unread} reports named files`} in a shape it does not recognise, so this says what this window could show, not that the chat changed nothing.`
              : 'No file changes in this chat. An app that edits from a shell command or a script reports no file list at all, so this is what it reported rather than everything a turn did.'}
          </p>
        )}
        {folded.empty === 'no-transcript' && (
          <p className="chg-empty">
            Nothing to read yet. This chat&apos;s transcript is not in this window, so its file
            changes are not here either. Open the chat once and they appear in this list.
          </p>
        )}
      </div>
    </div>
  )
}
