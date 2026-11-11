// The browser's info bars, in the pane's flow between the toolbar and the
// page (main's `.bw-strip` layout, so nothing floats over the page): a site
// asking for a permission.

import { useState } from 'react'
import { Bell, Camera, ClipboardText, Info, MapPin, Microphone, MusicNotes } from '@phosphor-icons/react'
import { hostOf, permissionAsk } from './browser-chrome.js'

const KIND_ICONS = {
  camera: Camera,
  microphone: Microphone,
  geolocation: MapPin,
  notifications: Bell,
  'clipboard-read': ClipboardText,
  midi: MusicNotes,
}

// "<host> wants to use your camera": Allow, Block, and whether to remember
// the answer for the site (never offered in a private tab, which keeps
// nothing).
export function PermissionBar({ prompt, onAnswer }) {
  const [remember, setRemember] = useState(false)
  const Icon = KIND_ICONS[prompt.kind] ?? Info
  const host = hostOf(prompt.origin) || prompt.origin || 'This site'
  return (
    <div className="bw-strip bw-info" role="alert">
      <Icon size={14} className="bw-strip-icon" />
      <span className="bw-info-text">
        <strong>{host}</strong> wants to {permissionAsk(prompt.kind, prompt.label)}
      </span>
      {!prompt.private && (
        <label className="bw-check">
          <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
          Remember for this site
        </label>
      )}
      <button type="button" className="pn-btn" onClick={() => onAnswer(false, remember)}>
        Block
      </button>
      <button type="button" className="pn-btn bw-primary" onClick={() => onAnswer(true, remember)}>
        Allow
      </button>
    </div>
  )
}
