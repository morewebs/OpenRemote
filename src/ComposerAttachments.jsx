// A composer's attachments: chips above its text - a picture's thumbnail,
// a picked element's picture and name - each with its own remove; and the
// ways in: paste, drop, and the attach button. The home screen's composer
// and each chat's use it. The items live in attachments.js, where the
// browser can add its own; the paths themselves are composer-attachments.js.

import { useRef, useSyncExternalStore } from 'react'
import { CursorClick, ImageSquare, Paperclip, X } from '@phosphor-icons/react'
import { attachments, removeAttachment, subscribe } from './attachments.js'
import { dragsFiles, droppedPictures, pastePlan, takePictures } from './composer-attachments.js'
import { consoleError, errorCopy } from './error-copy.js'

const NONE = []

// A file dropped anywhere else in the window must not replace the console
// with that file (a webview's default): only a composer takes files. A
// dragged chat (SplitArea) carries no files and is left alone.
if (typeof window !== 'undefined') {
  const holdFiles = (e) => {
    if (dragsFiles(e)) e.preventDefault()
  }
  window.addEventListener('dragover', holdFiles)
  window.addEventListener('drop', holdFiles)
}

/** A composer's attachments, live. */
export function useAttachments(key) {
  return useSyncExternalStore(subscribe, () => {
    const list = attachments(key)
    return list.length ? list : NONE
  })
}

function hostOf(url) {
  try {
    return new URL(url).host
  } catch {
    return url ?? ''
  }
}

function Chip({ item, off, why, onRemove }) {
  const element = item.kind === 'element'
  const name = element ? item.label || 'Element' : item.name || 'Picture'
  return (
    <div
      className={`ca-chip${element ? ' ca-chip--element' : ''}${off ? ' ca-chip--off' : ''}`}
      title={why ?? (element ? `${name} · ${hostOf(item.url)}` : name)}
    >
      {item.dataUrl ? (
        <img className="ca-thumb pv" src={item.dataUrl} alt="" draggable={false} />
      ) : (
        <span className="ca-thumb ca-thumb--none">
          {element ? <CursorClick size={14} /> : <ImageSquare size={14} />}
        </span>
      )}
      {element && (
        <span className="ca-label pv">
          <span className="ca-name">{name}</span>
          <span className="ca-where">{hostOf(item.url)}</span>
        </span>
      )}
      <button type="button" className="ca-remove" onClick={onRemove} title="Remove" aria-label={`Remove ${name}`}>
        <X size={11} weight="bold" />
      </button>
    </div>
  )
}

/**
 * The chips row above a composer's text. Where the app reads no pictures,
 * a picture's chip is dimmed and says why (a picked element still goes,
 * as words).
 */
export function AttachmentChips({ composerKey, takesImages = true, app = 'This app' }) {
  const items = useAttachments(composerKey)
  if (!items.length) return null
  return (
    <div className="ca-row">
      {items.map((item) => {
        const off = !takesImages && item.kind === 'image'
        return (
          <Chip
            key={item.id}
            item={item}
            off={off}
            why={off ? `${app} doesn't read pictures` : null}
            onRemove={() => removeAttachment(composerKey, item.id)}
          />
        )
      })}
    </div>
  )
}

/**
 * Paste, drop and the attach button for a composer: the handlers go on its
 * box, the button in its foot. Off (`enabled` false) where the app reads
 * no pictures: nothing is taken in and there is no button, and a picture
 * pasted or dropped there gets `refuse` (the sentence naming the app) when
 * one is given. `onError` gets errorCopy's sentence.
 */
export function useAttachInput(composerKey, { enabled, onError, refuse = null }) {
  const fileRef = useRef(null)
  const take = (files) => takePictures(composerKey, files, { onError })
  const refused = (e, files) => {
    if (!refuse || !files.length) return false
    e.preventDefault()
    onError?.(errorCopy(consoleError(refuse)))
    return true
  }
  // Words pasted with a picture go in as typed; only the picture is taken,
  // or refused, on its own.
  const onPaste = (e) => {
    const plan = pastePlan(e, { enabled, refuse })
    if (!plan.keepDefault) e.preventDefault()
    if (plan.refuse) onError?.(errorCopy(consoleError(refuse)))
    if (plan.take.length) take(plan.take)
  }
  const onDragOver = (e) => {
    if (!enabled || !dragsFiles(e)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
  }
  const onDrop = (e) => {
    const files = droppedPictures(e)
    if (!enabled) {
      refused(e, files)
      return
    }
    if (!files.length) return
    e.preventDefault()
    take(files)
  }
  // The picker takes any picture: kinds the apps don't read are drawn
  // again as one they do.
  const button = enabled ? (
    <>
      <input
        ref={fileRef}
        type="file"
        accept="image/*"
        multiple
        hidden
        onChange={(e) => {
          take([...e.target.files])
          e.target.value = ''
        }}
      />
      <button
        type="button"
        className="nc-meta ca-attach"
        onClick={() => fileRef.current?.click()}
        title="Attach pictures - or paste or drop them"
        aria-label="Attach pictures"
      >
        <Paperclip size={14} />
      </button>
    </>
  ) : null
  return { onPaste, onDragOver, onDrop, button }
}
