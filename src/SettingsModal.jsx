import { useEffect, useState } from 'react'
import AboutModal from './AboutModal.jsx'
import SignInModal from './SignInModal.jsx'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { loadDefaults, saveDefaults } from './defaults.js'
import { X } from '@phosphor-icons/react'
import './devices.css'
import './panels.css'
import './settingsmodal.css'

function loadRecentWorkspaces() {
  try {
    const raw = JSON.parse(localStorage.getItem('openremote-recent-workspaces') ?? '[]')
    return Array.isArray(raw) ? raw.filter((p) => typeof p === 'string') : []
  } catch {
    return []
  }
}

// The harnesses whose own login command the daemon can relay - the
// Sign-in action renders only there (no dead UI elsewhere).
const SIGNIN_HARNESSES = new Set(['claude', 'codex', 'grok'])

export default function SettingsModal({ onReplay, onClose }) {
  const { connection, capabilities, sessions, disconnect, modelsFor } = useConsole()
  const harnessList = capabilities?.harnesses ?? []
  const available = (harnessList ?? []).filter((h) => h.available)
  const [defaults, setDefaults] = useState(() => loadDefaults())
  const [models, setModels] = useState([])
  const [picker, setPicker] = useState(null)
  const [about, setAbout] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const recents = loadRecentWorkspaces()
  const defaultHarness = available.find((h) => h.id === defaults.harness) ?? available[0] ?? null
  const defaultWorkspace = defaults.workspace && recents.includes(defaults.workspace)
    ? defaults.workspace
    : recents[0] ?? null

  // The model row only renders where the default harness advertises -
  // the same rule the composer holds.
  useEffect(() => {
    let cancelled = false
    if (!defaultHarness) {
      setModels([])
      return
    }
    modelsFor(defaultHarness.id).then((list) => {
      if (!cancelled) setModels(list ?? [])
    })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [defaultHarness?.id, connection.state])

  const set = (partial) => {
    saveDefaults(partial)
    setDefaults(loadDefaults())
    setPicker(null)
  }

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const [reduceMotion, setReduceMotion] = useState(() => {
    try {
      return localStorage.getItem('openremote-reduce-motion') === '1'
    } catch {
      return false
    }
  })
  const [confirmDisconnect, setConfirmDisconnect] = useState(false)

  useEffect(() => {
    const onKey = (e) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const setReduce = (value) => {
    setReduceMotion(value)
    try {
      localStorage.setItem('openremote-reduce-motion', value ? '1' : '0')
    } catch {
      /* storage unavailable */
    }
    document.documentElement.toggleAttribute('data-reduce-motion', value)
  }

  return (
    <div
      className="dv-modal-backdrop stg-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="dv-modal stg-modal" role="dialog" aria-modal="true" aria-label="Settings">
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">Settings</h2>
          <button className="dv-modal-close" onClick={onClose} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">The daemon connection, the harness it drives, and this install.</p>
        <div className="panel stg-body">

      <h2 className="pn-section">New tasks</h2>
      <p className="pn-lead">What a new chat starts with. The composers still change their minds.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Harness</div>
            <div className="pn-detail">What a new task starts on.</div>
          </div>
          <button
            type="button"
            className="pn-btn"
            onClick={openPicker('harness')}
            aria-haspopup="listbox"
            aria-expanded={picker?.kind === 'harness'}
          >
            {defaultHarness?.name ?? 'No harness installed'}
          </button>
          {picker?.kind === 'harness' && (
            <PickerMenu
              label="Harness"
              searchPlaceholder="Search harnesses"
              items={available.map((h) => ({ id: h.id, name: h.name }))}
              groups={null}
              selectedId={defaultHarness?.id ?? ''}
              onChoose={(id) => set({ harness: id, model: null })}
              onClose={() => setPicker(null)}
              anchor={{ left: picker.x, top: picker.y }}
            />
          )}
        </div>
        {models.length > 0 && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Model</div>
              <div className="pn-detail">{defaultHarness?.name}'s own catalog.</div>
            </div>
            <button
              type="button"
              className="pn-btn"
              onClick={openPicker('model')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'model'}
            >
              {defaults.model && models.some((m) => m.model === defaults.model)
                ? defaults.model
                : `${defaultHarness?.name} default`}
            </button>
            {picker?.kind === 'model' && (
              <PickerMenu
                label="Model"
                searchPlaceholder="Search models"
                wide
                items={models.map((m) => ({
                  id: m.model,
                  name: m.display_name ?? m.model,
                  reasoning_efforts: m.reasoning_efforts,
                }))}
                groups={null}
                selectedId={defaults.model ?? ''}
                onChoose={(id) => set({ model: id })}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderSubline={(m) =>
                  m.reasoning_efforts?.length ? (
                    <span className="nc-item-sub">{m.reasoning_efforts.join(' · ')}</span>
                  ) : null
                }
              />
            )}
          </div>
        )}
        {recents.length > 0 && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Workspace</div>
              <div className="pn-detail">The folder a new task works in.</div>
            </div>
            <button
              type="button"
              className="pn-btn"
              onClick={openPicker('workspace')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'workspace'}
            >
              {defaultWorkspace
                ? defaultWorkspace.split(/[\\/]/).filter(Boolean).pop()
                : 'Choose a folder'}
            </button>
            {picker?.kind === 'workspace' && (
              <PickerMenu
                label="Workspace"
                searchPlaceholder="Search workspaces"
                wide
                items={recents.map((p) => ({
                  id: p,
                  name: p.split(/[\\/]/).filter(Boolean).pop(),
                }))}
                groups={null}
                selectedId={defaultWorkspace ?? ''}
                onChoose={(id) => set({ workspace: id })}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderSubline={(p) => <span className="nc-item-sub">{p.id}</span>}
              />
            )}
          </div>
        )}
      </div>

      <h2 className="pn-section">Daemon</h2>
      <p className="pn-lead">The local process the chats run through.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">{connection.state === 'connected' ? 'Connected' : 'Not connected'}</div>
            <div className="pn-detail">{connection.state === 'connected' ? 'Bearer token accepted.' : connection.error ?? 'Start the app or connect from the first-run steps.'}</div>
          </div>
          {confirmDisconnect ? (
            <div className="pn-actions">
              <button type="button" className="pn-btn" onClick={disconnect}>Disconnect</button>
              <button type="button" className="pn-btn" onClick={() => setConfirmDisconnect(false)}>Cancel</button>
            </div>
          ) : (
            <button
              type="button"
              className="pn-btn"
              onClick={() => setConfirmDisconnect(true)}
              disabled={connection.state !== 'connected'}
            >
              {connection.state === 'connected' ? 'Forget' : '-'}
            </button>
          )}
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">{(sessions ?? []).length} {(sessions ?? []).length === 1 ? 'chat' : 'chats'} on record</div>
            <div className="pn-detail">Sessions survive daemon restarts; stopped ones can resume.</div>
          </div>
        </div>
      </div>

      <h2 className="pn-section">Harnesses</h2>
      <p className="pn-lead">What the daemon can drive on this machine. Install a harness's own CLI and it appears here.</p>
      <div className="pn-list pn-list--tight">
        {(harnessList ?? []).map((h) => (
          <div className="pn-row" key={h.id}>
            <div>
              <div className="pn-name">{harnessName(h.id)}</div>
              <div className="pn-detail">{h.available ? h.path : 'Not found on this machine.'}</div>
            </div>
            {h.available && h.signed_in === false && SIGNIN_HARNESSES.has(h.id) ? (
              <button type="button" className="pn-btn" onClick={() => setSignInFor(h.id)}>
                Sign in
              </button>
            ) : (
              <span className={`pn-btn${h.available && h.signed_in !== false ? ' on' : ''}`}>
                {!h.available ? 'Missing' : h.signed_in === false ? 'Not signed in' : h.signed_in ? 'Signed in' : 'Ready'}
              </span>
            )}
          </div>
        ))}
      </div>

      <h2 className="pn-section">This install</h2>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">OpenRemote 0.1.0</div>
            <div className="pn-detail">Daemon {capabilities?.daemon ?? '-'} · the parity build</div>
          </div>
          <button type="button" className="pn-btn" onClick={() => setAbout(true)}>About</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Reduce motion</div>
            <div className="pn-detail">Stops the running-dot breathe and picker transitions.</div>
          </div>
          <button
            type="button"
            className={reduceMotion ? 'pn-btn on' : 'pn-btn'}
            onClick={() => setReduce(!reduceMotion)}
          >
            {reduceMotion ? 'On' : 'Off'}
          </button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">First-run steps</div>
            <div className="pn-detail">Shows them again. The chats stay.</div>
          </div>
          <button type="button" className="pn-btn" onClick={onReplay}>Show</button>
        </div>
      </div>

        </div>
        {about && <AboutModal onClose={() => setAbout(false)} daemonVersion={capabilities?.daemon ?? null} />}
        {signInFor && <SignInModal harnessId={signInFor} onDone={() => setSignInFor(null)} />}
      </div>
    </div>
  )
}
