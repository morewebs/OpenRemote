// The browser's info bars, in the pane's flow between the toolbar and the
// page (main's `.bw-strip` layout, so nothing floats over the page): a site
// asking for a permission, and a chat's agent using the tab. Also the tab
// strip's badge naming that chat.

import { useState } from 'react'
import { Bell, Camera, ClipboardText, Info, MapPin, Microphone, MusicNotes } from '@phosphor-icons/react'
import { hostOf, permissionAsk } from './browser-chrome.js'
import { usePrivacyMode } from './privacy.js'

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

// Which chat's agent holds a tab: its title on a small badge that breathes
// while the agent acts, still once the user took over. In privacy mode the
// title is blurred (`pv`) and the tooltip names no chat.
export function AgentBadge({ controller }) {
  const privacy = usePrivacyMode()
  const name = controller.title || 'A chat'
  const live = controller.kind === 'agent'
  const said = live ? `${name} is using this tab` : `You took over from ${name}`
  const quiet = live ? 'An agent is using this tab' : 'You took over from an agent'
  return (
    <span className={`bw-agent${live ? ' live' : ''}`} title={privacy ? quiet : said}>
      <span className="bw-agent-dot" />
      <span className="bw-agent-name pv">{name}</span>
    </span>
  )
}

// A chat's agent in this tab. "Take over" pauses the agent's actions here
// (it can still read the page); "Hand back" lets it go on; "Stop sharing"
// ends its hold.
export function AgentBar({ controller, onControl }) {
  const paused = controller.kind === 'paused'
  const name = controller.title || 'A chat'
  return (
    <div className="bw-strip bw-info" role="status">
      <span className={`bw-agent-dot${paused ? '' : ' live'}`} />
      <span className="bw-info-text">
        {paused ? (
          <>
            You took over from <strong className="pv">{name}</strong>
          </>
        ) : (
          <>
            <strong className="pv">{name}</strong> is using this tab
          </>
        )}
      </span>
      {paused ? (
        <>
          <button type="button" className="pn-btn" onClick={() => onControl('release')}>
            Stop sharing
          </button>
          <button type="button" className="pn-btn bw-primary" onClick={() => onControl('hand_back')}>
            Hand back
          </button>
        </>
      ) : (
        <button type="button" className="pn-btn bw-primary" onClick={() => onControl('take_over')}>
          Take over
        </button>
      )}
    </div>
  )
}
