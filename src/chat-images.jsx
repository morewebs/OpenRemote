// Pictures in a chat - what the user attached, what another agent sent with
// its brief or report, what an agent's tool answered with (a screenshot of a
// page) - live in the blob store of the device that runs the chat, never in
// the transcript. Each is fetched with the token (an <img> can't send one)
// into an object URL that lasts as long as the console and is shared by
// every card showing it: a picture is named by its hash, so it never changes.

import { useEffect, useRef, useState } from 'react'
import { ImageBroken, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { pictureKey, pictureMissing } from './chat-pictures.js'
import './devices.css'

const urls = new Map()

function objectUrl(api, image, session) {
  const key = pictureKey(api.prefix, session, image.blob)
  let url = urls.get(key)
  if (!url) {
    url = api.blob(image.blob, { session, mime: image.mime }).then((bytes) => URL.createObjectURL(bytes))
    // A picture that couldn't be fetched (its device offline) is asked
    // for again the next time it shows.
    url.catch(() => urls.delete(key))
    urls.set(key, url)
  }
  return url
}

/**
 * A chat's picture as an object URL: `{ url, failed }`, url null while it
 * loads, failed the sentence when it could not be fetched. A chat that runs
 * on another device is asked through that device (its chat-scoped route
 * answers only that chat's pictures).
 */
export function useChatImage(image, chat) {
  const { apiFor, thisDevice } = useConsole()
  const elsewhere = chat?.executor && chat.executor !== thisDevice ? chat.executor : null
  const [state, setState] = useState({ url: null, failed: null })
  useEffect(() => {
    let live = true
    setState({ url: null, failed: null })
    objectUrl(apiFor(elsewhere), image, elsewhere ? chat.id : null)
      .then((url) => live && setState({ url, failed: null }))
      .catch((err) => live && setState({ url: null, failed: pictureMissing(err, Boolean(elsewhere)) }))
    return () => {
      live = false
    }
    // The fetch follows the picture and where the chat runs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [image.blob, elsewhere, chat?.id, apiFor])
  return state
}

function Thumb({ image, chat, label, onOpen }) {
  const { url, failed } = useChatImage(image, chat)
  if (failed) {
    return (
      <span className="ci-missing" title={failed}>
        <ImageBroken size={14} />
        Picture unavailable
      </span>
    )
  }
  return (
    <button type="button" className="ci-thumb" onClick={onOpen} disabled={!url} title="See it large">
      {url ? <img className="pv" src={url} alt={label} draggable={false} /> : <span className="ci-wait" />}
    </button>
  )
}

/** The large view: Esc, the backdrop or X closes it, like the other modals. */
function ImageViewer({ image, chat, label, onClose }) {
  const { url } = useChatImage(image, chat)
  const panelRef = useRef(null)
  useBack(true, onClose)
  useDialog(panelRef, onClose)
  // The modal family's backdrop: a browser page beside the chat steps out
  // of view for it (browser-occlusion.js lists it).
  return (
    <div
      className="dv-modal-backdrop ci-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="ci-viewer" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={label}>
        <button type="button" className="dv-modal-close ci-close" onClick={onClose} title="Close">
          <X size={14} weight="bold" />
        </button>
        {url && <img className="pv" src={url} alt={label} />}
      </div>
    </div>
  )
}

/**
 * A row of a message's or a tool result's pictures, each opening large on
 * a click. `label` names them for screen readers ("Screenshot").
 */
export function ChatImages({ images, chat, label = 'Picture' }) {
  const [open, setOpen] = useState(null)
  if (!images?.length) return null
  return (
    <div className="ci-strip">
      {images.map((image, i) => (
        <Thumb
          key={`${image.blob}-${i}`}
          image={image}
          chat={chat}
          label={images.length > 1 ? `${label} ${i + 1}` : label}
          onOpen={() => setOpen(image)}
        />
      ))}
      {open && <ImageViewer image={open} chat={chat} label={label} onClose={() => setOpen(null)} />}
    </div>
  )
}

/** A kept picture as a data URL, for putting it back in a composer. */
export async function chatImageDataUrl(api, image, session = null) {
  const bytes = await api.blob(image.blob, { session, mime: image.mime })
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result)
    reader.onerror = () => reject(reader.error)
    reader.readAsDataURL(bytes)
  })
}
