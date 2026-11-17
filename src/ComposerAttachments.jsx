// A composer's attachments: chips above its text - a picture's thumbnail,
// a picked element's picture and name - each with its own remove; and the
// ways in: paste, drop, and the attach button. The home screen's composer
// and each chat's use it. The items live in attachments.js, where the
// browser can add its own; the paths themselves are composer-attachments.js.

import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { CursorClick, ImageSquare, FileText, Hourglass, WarningCircle, X } from '@phosphor-icons/react'
import { attachments, removeAttachment, subscribe } from './attachments.js'
import { dragsFiles, pastePlan } from './composer-attachments.js'
import { consoleError, errorCopy } from './error-copy.js'
import { takeFiles, cancelImports, retryImport, FILE_ACCEPT } from './document-input.js'
import { usePluginComposerActions } from './PluginComposerButtons.jsx'
import ComposerAddMenu from './ComposerAddMenu.jsx'
import DocumentPreview from './DocumentPreview.jsx'
import { composerTarget } from './composer-insert.js'
import { consumeRecoveredDraft, onRecoveredDraft } from './composer-recovery.js'

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
  const [preview, setPreview] = useState(null)
  if (!items.length) return null
  return (
    <><div className="ca-row" aria-live="polite">
      {items.map((item) => {
        const off = !takesImages && item.kind === 'image'
        if (item.kind === 'document' || item.status === 'pending' || item.status === 'error') {
          const ready = item.status === 'ready'
          const Icon = item.status === 'pending' ? Hourglass : item.status === 'error' ? WarningCircle : FileText
          return <div className="ca-chip ca-document" data-status={item.status} key={item.id}>
            <button type="button" className="ca-document-open" disabled={!ready} onClick={() => setPreview(item.id)} aria-label={`Preview ${item.name}`} title={item.error || (ready ? 'Preview the text included with your message' : 'Preparing attachment')}>
              <span className="ca-thumb ca-thumb--none"><Icon size={20} /></span>
              <span className="ca-label pv"><span className="ca-name">{item.name}</span><span className="ca-where">{item.status === 'pending' ? 'Preparing…' : item.status === 'error' ? item.error : `${item.format} · ${item.text?.length.toLocaleString()} characters`}</span></span>
            </button>
            {item.status === 'error' && <button type="button" className="ca-retry" onClick={() => retryImport(composerKey, item.id)}>Retry</button>}
            <button type="button" className="ca-remove" onClick={() => removeAttachment(composerKey, item.id)} aria-label={`Remove ${item.name}`} title="Remove"><X size={12} /></button>
          </div>
        }
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
    </div>{preview && items.find(i => i.id === preview && i.status === 'ready') && <DocumentPreview item={items.find(i => i.id === preview)} onClose={() => setPreview(null)} />}</>
  )
}

/**
 * Paste, drop and the attach button for a composer: the handlers go on its
 * box, the Add menu in its foot. `enabled` controls image input; documents
 * and text actions remain available to text-only Apps. `disabled` follows
 * whether this composer can currently be edited.
 */
export function useAttachInput(composerKey, { enabled, onError, refuse = null, disabled = false, inputRef, setText, workspace = null, projectId, deviceId, onReference, onCommands, onPlugins }) {
  const fileRef = useRef(null)
  const photoRef = useRef(null)
  const cameraRef = useRef(null)
  const pluginActions = usePluginComposerActions({ inputRef, setText, composer: composerKey, workspace, projectId, deviceId, disabled })
  const take = (files) => takeFiles(composerKey, files, { images: enabled, onError })
  useEffect(() => () => cancelImports(composerKey), [composerKey])
  useEffect(() => {
    const restore = (key) => {
      if (key !== composerKey) return
      const recovered = consumeRecoveredDraft(key)
      if (recovered) setText(current => current.trim() ? `${recovered}\n\n${current}` : recovered)
    }
    restore(composerKey)
    return onRecoveredDraft(restore)
  }, [composerKey, setText])
  // Words pasted with a picture go in as typed; only the picture is taken,
  // or refused, on its own.
  const onPaste = (e) => {
    if (disabled) return
    const plan = pastePlan(e, { enabled, refuse })
    if (!plan.keepDefault) e.preventDefault()
    if (plan.refuse) onError?.(errorCopy(consoleError(refuse)))
    if (plan.take.length) take(plan.take)
  }
  const onDragOver = (e) => {
    if (disabled || !dragsFiles(e)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
  }
  const onDrop = (e) => {
    if (disabled) return
    const files = [...(e.dataTransfer?.files ?? [])]
    if (!files.length) return
    e.preventDefault()
    take(files)
  }
  // The picker takes any picture: kinds the apps don't read are drawn
  // again as one they do.
  const pasteClipboard = async () => {
    const target = composerTarget(composerKey)
    try {
      const files = []
      if (navigator.clipboard?.read) {
        const items = await navigator.clipboard.read()
        for (const item of items) {
          const type = item.types.find(t => t.startsWith('image/')) || item.types.find(t => t === 'text/plain')
          if (type) files.push(new File([await item.getType(type)], type.startsWith('image/') ? 'Clipboard photo.png' : 'Clipboard.txt', { type }))
        }
      } else {
        const text = await navigator.clipboard.readText()
        if (text) files.push(new File([text], 'Clipboard.txt', { type: 'text/plain' }))
      }
      if (!target?.isLive()) return
      if (!files.length) onError?.('The clipboard has no readable text or photos. Copy something, then try again.')
      else await take(files)
    } catch { if (target?.isLive()) onError?.('Clipboard access was not available. Paste directly into the message with your keyboard instead.') }
  }
  const picked = (e) => {
    if (!disabled) take([...e.target.files])
    e.target.value = ''
  }
  const button = (
    <>
      <input
        ref={fileRef}
        type="file"
        accept={FILE_ACCEPT}
        multiple
        hidden
        onChange={picked}
      />
      <input ref={photoRef} type="file" accept="image/*" multiple hidden onChange={picked} />
      <input ref={cameraRef} type="file" accept="image/*" capture="environment" hidden onChange={picked} />
      <ComposerAddMenu composerKey={composerKey} takesImages={enabled} disabled={disabled} pickPhoto={() => photoRef.current?.click()} pickFile={() => fileRef.current?.click()} pickCamera={() => cameraRef.current?.click()} pasteClipboard={pasteClipboard} onReference={onReference} onCommands={onCommands} onPlugins={onPlugins} pluginActions={pluginActions} onError={onError} />
    </>
  )
  return { onPaste, onDragOver, onDrop, button, pluginActions }
}
