import { useEffect, useRef, useState } from 'react'
import { CaretLeft, CaretRight, X } from '@phosphor-icons/react'
// The one copy of the version the UI renders - package.json is the
// source; tauri.conf.json and src-tauri/Cargo.toml carry it for the
// installer side.
import { version } from '../package.json'
import './about.css'
import { isMobile } from './platform.js'
import { SplashMark } from './Splash.jsx'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'

export default function AboutModal({ onClose, daemonVersion, api }) {
  const [view, setView] = useState('about')
  useBack(true, () => {
    if (view === 'licenses') setView('about')
    else onClose()
  })
  const panelRef = useRef(null)
  // Esc closes this window, never the Settings window it was opened over:
  // that one holds a layer of its own underneath this. The panel ref is empty
  // on the phone, where this renders as a full screen; the Esc layer still
  // holds, and the screen itself owns its focus.
  useDialog(panelRef, onClose)
  const [notices, setNotices] = useState(null)
  const [noticesError, setNoticesError] = useState(null)
  // The daemon serves the notices itself (GET /notices), so the text is
  // whatever this build carries - the same file the installers bundle
  // beside the app. Fetched once, only when the licenses are asked for.
  useEffect(() => {
    if (view !== 'licenses' || notices !== null || noticesError !== null || !api?.ready) {
      return
    }
    let stopped = false
    api
      .notices()
      .then((text) => {
        if (!stopped) setNotices(text)
      })
      .catch(() => {
        if (!stopped) setNoticesError('The licenses could not be loaded.')
      })
    return () => {
      stopped = true
    }
  }, [view, notices, noticesError, api])

  if (isMobile) {
    return (
      <AboutScreen
        view={view}
        version={version}
        daemonVersion={daemonVersion}
        notices={notices}
        noticesError={noticesError}
        onClose={onClose}
        onLicenses={() => setView('licenses')}
        onBack={() => setView('about')}
      />
    )
  }

  return (
    <div
      className="ab-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className={view === 'licenses' ? 'ab-modal ab-modal--wide' : 'ab-modal'}
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={view === 'licenses' ? 'Third-party licenses' : 'About OpenRemote'}
      >
        <div className="ab-head">
          <h2 className="ab-wordmark">{view === 'licenses' ? 'Third-party licenses' : 'OpenRemote'}</h2>
          <button className="ab-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        {view === 'about' ? (
          <>
            <div className="ab-version">Version {version} · slice 1</div>
            {daemonVersion && <div className="ab-version">Background service {daemonVersion}</div>}
            <div className="ab-sep" />
            <button type="button" className="ab-btn ab-btn--block" onClick={() => setView('licenses')}>
              Third-party licenses
            </button>
          </>
        ) : noticesError !== null ? (
          <>
            <div className="ab-notices-error">{noticesError}</div>
            <button type="button" className="ab-btn ab-btn--block" onClick={() => setView('about')}>
              Back
            </button>
          </>
        ) : notices !== null ? (
          <>
            <pre className="ab-notices" aria-label="Third-party notices">{notices}</pre>
            <button type="button" className="ab-btn ab-btn--block" onClick={() => setView('about')}>
              Back
            </button>
          </>
        ) : (
          <>
            <div className="ab-notices-loading">Loading the licenses…</div>
            <button type="button" className="ab-btn ab-btn--block" onClick={() => setView('about')}>
              Back
            </button>
          </>
        )}
      </div>
    </div>
  )
}

function AboutScreen({
  view,
  version,
  daemonVersion,
  notices,
  noticesError,
  onClose,
  onLicenses,
  onBack,
}) {
  const title = view === 'licenses' ? 'Third-party licenses' : 'About'
  const back = view === 'licenses' ? onBack : onClose
  return (
    <div className="ab-screen" role="dialog" aria-modal="true" aria-label={title}>
      <header className="ab-toolbar">
        <button type="button" className="ab-back" onClick={back} aria-label="Back">
          <CaretLeft size={22} />
        </button>
        <h1 className="ab-toolbar-title">{title}</h1>
      </header>
      {view === 'about' ? (
        <div className="ab-body">
          <div className="ab-identity">
            <SplashMark className="ab-mark" />
            <p className="ab-name">OpenRemote</p>
          </div>
          <div className="ab-list">
            <div className="ab-row">
              <span className="ab-row-label">Version</span>
              <span className="ab-row-value">{version}</span>
            </div>
            {daemonVersion && (
              <div className="ab-row">
                <span className="ab-row-label">Background service</span>
                <span className="ab-row-value">{daemonVersion}</span>
              </div>
            )}
            <button type="button" className="ab-row ab-row--link" onClick={onLicenses}>
              <span className="ab-row-copy">
                <span className="ab-row-label">Third-party licenses</span>
                <span className="ab-row-value">Notices bundled with this build</span>
              </span>
              <CaretRight size={18} />
            </button>
          </div>
        </div>
      ) : (
        <div className="ab-body ab-body--licenses">
          {noticesError !== null ? (
            <p className="ab-notices-error">{noticesError}</p>
          ) : notices !== null ? (
            <pre className="ab-notices ab-notices--screen" aria-label="Third-party notices">{notices}</pre>
          ) : (
            <p className="ab-notices-loading">Loading the licenses…</p>
          )}
        </div>
      )}
    </div>
  )
}
