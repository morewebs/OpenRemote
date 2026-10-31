import { useEffect, useState } from 'react'
import { ArrowLeft, ArrowRight, Info, SidebarSimple } from '@phosphor-icons/react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import AboutModal from './AboutModal.jsx'
import SplitGlyph from './SplitGlyph.jsx'
import { useConsole } from './state/console.jsx'
import { isMobile } from './platform.js'
import { isMac, useHint } from './keyboard.jsx'
import './titlebar.css'

const hasTauri = typeof window !== 'undefined' && !!window.__TAURI_INTERNALS__

// The caption glyphs are drawn, not taken from Segoe MDL2: on a real
// Windows 10 box the font resolved U+E8E7 through fallback to mojibake
// (an "A" with an accent) while E921/E8BB rendered fine — per-codepoint
// font roulette no button should ride. The shapes below are MDL2's own
// 10px geometry, so every system draws the same caption buttons.

// macOS draws its own traffic lights over the bar's left end (the overlay
// titlebar, tauri.macos.conf.json), so the bar leaves them room and has
// no caption buttons of its own there.
const MAC = isMac && !isMobile

// Two bars, not three: the shorter menu mark, drawn a little heavier than
// a hairline hamburger so it reads as a control at the phone's bar size.
function MenuGlyph() {
  return (
    <svg className="tb-menu" width="20" height="14" viewBox="0 0 18 14" aria-hidden="true">
      <path d="M1.2 4.2h15.6M1.2 9.8h15.6" />
    </svg>
  )
}

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
  onSplit,
}) {
  const { capabilities } = useConsole()
  const hintSidebar = useHint('toggle-sidebar')
  const hintSplit = useHint('split-right')
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
            title={hintSidebar(sidebarOpen ? 'Close the sidebar' : 'Open the sidebar')}
            aria-label={sidebarOpen ? 'Close the sidebar' : 'Open the sidebar'}
          >
            <MenuGlyph />
          </button>
        )}
        <div className="tb-title">OpenRemote</div>
        <button
          className="tb-toggle"
          onClick={() => setAbout(true)}
          title="About OpenRemote"
          aria-label="About OpenRemote"
        >
          <Info size={18} weight="duotone" />
        </button>
        {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
      </header>
    )
  }

  return (
    <header className={MAC ? 'titlebar is-mac' : 'titlebar'} data-tauri-drag-region>
      <div className="tb-left" data-tauri-drag-region>
        {onToggleSidebar && (
          <button
            className="tb-toggle"
            onClick={onToggleSidebar}
            title={hintSidebar(sidebarOpen ? 'Collapse sidebar' : 'Expand sidebar')}
          >
            <SidebarSimple size={16} weight="duotone" />
          </button>
        )}
        <button className="tb-toggle" onClick={onBack} disabled={!canBack} title="Back">
          <ArrowLeft size={16} weight="bold" />
        </button>
        <button className="tb-toggle" onClick={onForward} disabled={!canForward} title="Forward">
          <ArrowRight size={16} weight="bold" />
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
          <Info size={14} weight="duotone" />
        </button>
      </div>
      {/* The split sits at the bar's right end, where VS Code keeps its own
          split-editor button: next to the caption buttons on Windows, at the
          edge on macOS (which draws no caption buttons). Another pane beside
          this one is one click away (Ctrl+\; Ctrl+Shift+\ splits downward). */}
      <div className="tb-right">
        {onSplit && (
          <button className="tb-toggle" onClick={onSplit} title={hintSplit('Split view')}>
            <SplitGlyph />
          </button>
        )}
        {!MAC && (
        <div className="tb-controls">
          <button className="tb-btn" onClick={action('minimize')} title="Minimize">
            {<CaptionGlyph kind="minimize" />}
          </button>
          <button
            className="tb-btn"
            onClick={action('toggleMaximize')}
            title={maximized ? 'Restore' : 'Maximize'}
          >
            <CaptionGlyph kind={maximized ? 'restore' : 'maximize'} />
          </button>
          <button className="tb-btn tb-close" onClick={action('close')} title="Close">
            <CaptionGlyph kind="close" />
          </button>
        </div>
        )}
      </div>
      {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
    </header>
  )
}
