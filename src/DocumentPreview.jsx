import { useRef } from 'react'
import { createPortal } from 'react-dom'
import { X } from '@phosphor-icons/react'
import { useDialog } from './dialog-focus.js'
import { useBack } from './use-back.js'
import './devices.css'
import './composer-add.css'

export default function DocumentPreview({ item, onClose }) {
  const panelRef = useRef(null)
  useDialog(panelRef, onClose)
  useBack(true, onClose)
  return createPortal(<div className="dv-modal-backdrop" onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}>
    <div className="dv-modal ca-document-preview" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`Preview ${item.name}`}>
      <div className="dv-modal-head"><h2 className="pv">{item.name}</h2><button type="button" className="dv-modal-x" onClick={onClose} aria-label="Close preview"><X size={18} /></button></div>
      <p>{item.format || 'Text'} · {item.text.length.toLocaleString()} characters. This text will be included with your message. Images, formatting and charts are not included.</p>
      <pre className="pv">{item.text}</pre>
    </div>
  </div>, document.body)
}
