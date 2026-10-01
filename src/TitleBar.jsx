import { useEffect, useState } from 'react'
import { ArrowLeft, ArrowRight, Info, SidebarSimple } from '@phosphor-icons/react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import AboutModal from './AboutModal.jsx'
import './titlebar.css'

const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

export default function TitleBar({
  onToggleSidebar,
  sidebarOpen,
  onBack,
  onForward,
  canBack,
  canForward,
  version,
  latest,
  reduceMotion,
  onUpdated,
}) {
  const [maximized, setMaximized] = useState(false)
  const [about, setAbout] = useState(false)

  useEffect(() => {
    if (!hasTauri) return
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

  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="tb-left" data-tauri-drag-region>
        <button
          className="tb-toggle"
          onClick={onToggleSidebar}
          title={sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar'}
        >
          <SidebarSimple size={15} />
        </button>
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
          {'\uE921'}
        </button>
        <button
          className="tb-btn"
          onClick={action('toggleMaximize')}
          title={maximized ? 'Restore' : 'Maximize'}
        >
          {maximized ? '\uE923' : '\uE8E7'}
        </button>
        <button className="tb-btn tb-close" onClick={action('close')} title="Close">
          {'\uE8BB'}
        </button>
      </div>
      {about && (
        <AboutModal
          onClose={() => setAbout(false)}
          version={version}
          latest={latest}
          reduceMotion={reduceMotion}
          onUpdated={onUpdated}
        />
      )}
    </header>
  )
}

