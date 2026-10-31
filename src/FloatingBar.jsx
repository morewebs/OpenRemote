// The homepage's floating shortcut bar: one round bar pinned to the top
// right corner of the new-chat view. It holds the browser for now - the
// bar is the home for whatever shortcuts come next.

import { Globe } from '@phosphor-icons/react'
import './floating-bar.css'

export default function FloatingBar({ browserOn, onBrowser }) {
  // A build without the shell has no embedded browser to open, so the bar
  // offers nothing rather than a button that cannot act.
  if (!onBrowser) return null
  return (
    <div className="fb-bar" role="toolbar" aria-label="Shortcuts">
      <button
        type="button"
        className={`fb-btn${browserOn ? ' on' : ''}`}
        title="Browser - a side pane beside the chat"
        aria-label="Browser"
        aria-pressed={browserOn}
        onClick={onBrowser}
      >
        <Globe size={20} />
      </button>
    </div>
  )
}
