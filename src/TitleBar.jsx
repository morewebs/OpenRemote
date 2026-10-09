import { useEffect, useState } from 'react'
import { ArrowLeft, ArrowRight, Info, SidebarSimple } from '@phosphor-icons/react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import AboutModal from './AboutModal.jsx'
import { useConsole } from './state/console.jsx'
import { isMobile } from './platform.js'
import './titlebar.css'

const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// Windows draws its caption glyphs from Segoe MDL2 (the contract). Other
// systems don't have that font, so the same 10px shapes are drawn here.
const MDL2 = typeof navigator !== 'undefined' && /Windows/.test(navigator.userAgent)

function CaptionGlyph({ kind }) {
  return (
    <svg className="tb-glyph" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
      {kind === 'minimize' && <path d="M0 5.5h10" />}
      {kind === 'maximize' && <rect x="0.5" y="0.5" width="9" height="9" />}
      {kind === 'restore' && (
        <>
          <rect x="0.5" y="2.5" width="7" height="7" />
          <path d="M2.5 2.5V0.5h7v7h-2" />
        </>
      )}
      {kind === 'close' && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />}
    </svg>
  )
}

export default function TitleBar({
  onToggleSidebar,
  sidebarOpen,
  onBack,
  onForward,
  canBack,
  canForward,
}) {
  const { capabilities } = useConsole()
  const [maximized, setMaximized] = useState(false)
  const [about, setAbout] = useState(false)

  useEffect(() => {
    if (!hasTauri || isMobile) return
    const win = getCurrentWindow()
    let disposed = false
    const sync = async () => {
      try {
        setMaximized(await win.isMaximized())
      } catch {
        /* window gone */
      }
    }
    sync()
    const unlisten = win.onResized(() => {
      if (!disposed) sync()
    })
    return () => {
      disposed = true
      unlisten.then((fn) => fn()).catch(() => {})
    }
  }, [])

  const action = (method) => () => {
    if (hasTauri) getCurrentWindow()[method]()
  }

  // A phone has no window to drag, size or close, and its own back button:
  // the bar keeps the drawer's toggle, the name, and About.
  if (isMobile) {
    return (
      <header className="titlebar is-mobile">
        {onToggleSidebar && (
          <button
            className="tb-toggle"
            onClick={onToggleSidebar}
            title={sidebarOpen ? 'Close the sidebar' : 'Open the sidebar'}
            aria-label={sidebarOpen ? 'Close the sidebar' : 'Open the sidebar'}
          >
            <SidebarSimple size={20} />
          </button>
        )}
        <div className="tb-title">OpenRemote</div>
        <button
          className="tb-toggle"
          onClick={() => setAbout(true)}
          title="About OpenRemote"
          aria-label="About OpenRemote"
        >
          <Info size={18} />
        </button>
        {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
      </header>
    )
  }

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-left" data-tauri-drag-region>
        {onToggleSidebar && (
          <button
            className="tb-toggle"
            onClick={onToggleSidebar}
            title={sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar'}
          >
            <SidebarSimple size={15} />
          </button>
        )}
        <button className="tb-toggle" onClick={onBack} disabled={!canBack} title="Back">
          <ArrowLeft size={15} />
        </button>
        <button className="tb-toggle" onClick={onForward} disabled={!canForward} title="Forward">
          <ArrowRight size={15} />
        </button>
      </div>
      <div className="tb-title" data-tauri-drag-region>
        OpenRemote
        <button
          className="tb-info"
          onClick={() => setAbout(true)}
          title="About OpenRemote"
          aria-label="About OpenRemote"
        >
          <Info size={12} />
        </button>
      </div>
      <div className="tb-controls">
        <button className="tb-btn" onClick={action('minimize')} title="Minimize">
          {MDL2 ? '\uE921' : <CaptionGlyph kind="minimize" />}
        </button>
        <button
          className="tb-btn"
          onClick={action('toggleMaximize')}
          title={maximized ? 'Restore' : 'Maximize'}
        >
          {MDL2 ? (maximized ? '\uE923' : '\uE8E7') : <CaptionGlyph kind={maximized ? 'restore' : 'maximize'} />}
        </button>
        <button className="tb-btn tb-close" onClick={action('close')} title="Close">
          {MDL2 ? '\uE8BB' : <CaptionGlyph kind="close" />}
        </button>
      </div>
      {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
    </header>
  )
}
