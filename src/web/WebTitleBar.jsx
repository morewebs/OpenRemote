import { useState } from 'react'
import { ArrowLeft, ArrowRight, Info, SidebarSimple } from '@phosphor-icons/react'
import AboutModal from '../AboutModal.jsx'
import { useConsole } from '../state/console.jsx'

// The desktop's titlebar without Tauri: no drag region, no caption buttons,
// and no window API. The sidebar toggle, in-app back/forward and the name
// are the whole bar.
export default function WebTitleBar({
  sidebarOpen,
  onToggleSidebar,
  onBack,
  onForward,
  canBack,
  canForward,
}) {
  const { capabilities } = useConsole()
  const [about, setAbout] = useState(false)
  return (
    <header className="web-titlebar">
      <div className="web-tb-left">
        <button
          type="button"
          className="web-tb-toggle"
          onClick={onToggleSidebar}
          title={sidebarOpen ? 'Collapse the sidebar' : 'Expand the sidebar'}
          aria-label={sidebarOpen ? 'Collapse the sidebar' : 'Expand the sidebar'}
        >
          <SidebarSimple size={16} weight="duotone" />
        </button>
        <button
          type="button"
          className="web-tb-toggle"
          onClick={onBack}
          disabled={!canBack}
          title="Back"
          aria-label="Back"
        >
          <ArrowLeft size={16} weight="bold" />
        </button>
        <button
          type="button"
          className="web-tb-toggle"
          onClick={onForward}
          disabled={!canForward}
          title="Forward"
          aria-label="Forward"
        >
          <ArrowRight size={16} weight="bold" />
        </button>
      </div>
      <div className="web-tb-title">
        OpenRemote
        <button
          type="button"
          className="web-tb-info"
          onClick={() => setAbout(true)}
          title="About OpenRemote"
          aria-label="About OpenRemote"
        >
          <Info size={14} weight="duotone" />
        </button>
      </div>
      <div className="web-tb-right" />
      {about && (
        <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />
      )}
    </header>
  )
}
