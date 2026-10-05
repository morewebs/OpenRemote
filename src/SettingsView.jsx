import { useEffect, useState } from 'react'
import AboutModal from './AboutModal.jsx'
import SignInModal from './SignInModal.jsx'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { loadDefaults, saveDefaults } from './defaults.js'
import {
  loadPrefs,
  savePrefs,
  applyReduceMotion,
  applyDensity,
  loadRecentWorkspaces,
  pushRecentWorkspace,
} from './settings.js'
import { canPickFolder, pickFolder } from './pick-folder.js'
import { version } from '../package.json'
import './devices.css'
import './panels.css'
import './settings.css'

// The harnesses whose own login command the daemon can relay - the
// Sign-in action renders only there (no dead UI elsewhere).
const SIGNIN_HARNESSES = new Set(['claude', 'codex', 'grok'])

// The rail's contract: one anchor per section, in panel order. The ids
// match the section elements, so the rail stays honest as sections
// change - add a section, add its anchor here.
const SECTIONS = [
  { id: 'new-tasks', label: 'New tasks' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'startup', label: 'Startup' },
  { id: 'daemon', label: 'Daemon' },
  { id: 'harnesses', label: 'Harnesses' },
  { id: 'this-install', label: 'This install' },
]

export default function SettingsView({ onReplay }) {
  const { connection, capabilities, sessions, disconnect, modelsFor } = useConsole()
  const harnessList = capabilities?.harnesses ?? []
  const available = (harnessList ?? []).filter((h) => h.available)
  const [defaults, setDefaults] = useState(() => loadDefaults())
  const [models, setModels] = useState([])
  const [picker, setPicker] = useState(null)
  const [about, setAbout] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [prefs, setPrefs] = useState(() => loadPrefs())
  const [confirmDisconnect, setConfirmDisconnect] = useState(false)
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

  const setPref = (partial) => {
    savePrefs(partial)
    setPrefs(loadPrefs())
  }

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  // A fresh folder is one dialog away even when recents are empty -
  // the row never hides.
  const browse = async () => {
    const path = await pickFolder()
    if (!path) return
    pushRecentWorkspace(path)
    set({ workspace: path })
  }

  const toggleReduce = () => {
    const value = !prefs.reduce_motion
    setPref({ reduce_motion: value })
    applyReduceMotion(document, value)
  }

  return (
    <div className="settings">
      <nav className="stg-nav" aria-label="Settings sections">
        {SECTIONS.map((s) => (
          <a key={s.id} href={`#stg-${s.id}`} className="stg-nav-btn">
            {s.label}
          </a>
        ))}
      </nav>
      <div className="panel stg-panel">

      <h2 className="pn-section" id="stg-new-tasks">New tasks</h2>
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
        <div className="pn-row">
          <div>
            <div className="pn-name">Workspace</div>
            <div className="pn-detail">The folder a new task works in.</div>
          </div>
          <div className="pn-actions">
            {canPickFolder && (
              <button type="button" className="pn-btn" onClick={browse}>
                Browse
              </button>
            )}
            <button
              type="button"
              className="pn-btn"
              onClick={recents.length ? openPicker('workspace') : canPickFolder ? browse : undefined}
              disabled={!recents.length && !canPickFolder}
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
        </div>
      </div>

      <h2 className="pn-section" id="stg-appearance">Appearance</h2>
      <p className="pn-lead">Space, not themes - the dark theme is the contract.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Density</div>
            <div className="pn-detail">Compact pulls the rows and gutters in.</div>
          </div>
          <button
            type="button"
            className={prefs.density === 'compact' ? 'pn-btn on' : 'pn-btn'}
            onClick={() => {
              const value = prefs.density === 'compact' ? 'comfortable' : 'compact'
              setPref({ density: value })
              applyDensity(document, value)
            }}
          >
            {prefs.density === 'compact' ? 'Compact' : 'Comfortable'}
          </button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Reduce motion</div>
            <div className="pn-detail">Stops the running-dot breathe and picker transitions.</div>
          </div>
          <button
            type="button"
            className={prefs.reduce_motion ? 'pn-btn on' : 'pn-btn'}
            onClick={toggleReduce}
          >
            {prefs.reduce_motion ? 'On' : 'Off'}
          </button>
        </div>
      </div>

      <h2 className="pn-section" id="stg-startup">Startup</h2>
      <p className="pn-lead">What the app does when it opens.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Reopen</div>
            <div className="pn-detail">The view you left, or always New chat.</div>
          </div>
          <button
            type="button"
            className="pn-btn"
            onClick={openPicker('startup')}
            aria-haspopup="listbox"
            aria-expanded={picker?.kind === 'startup'}
          >
            {prefs.startup_view === 'last' ? 'Last view' : 'New chat'}
          </button>
          {picker?.kind === 'startup' && (
            <PickerMenu
              label="Reopen"
              searchPlaceholder="Search"
              items={[
                { id: 'new', name: 'New chat' },
                { id: 'last', name: 'Last view' },
              ]}
              groups={null}
              selectedId={prefs.startup_view}
              onChoose={(id) => setPref({ startup_view: id })}
              onClose={() => setPicker(null)}
              anchor={{ left: picker.x, top: picker.y }}
            />
          )}
        </div>
      </div>

      <h2 className="pn-section" id="stg-daemon">Daemon</h2>
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

      <h2 className="pn-section" id="stg-harnesses">Harnesses</h2>
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

      <h2 className="pn-section" id="stg-this-install">This install</h2>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">OpenRemote {version}</div>
            <div className="pn-detail">Daemon {capabilities?.daemon ?? '-'} · the parity build</div>
          </div>
          <button type="button" className="pn-btn" onClick={() => setAbout(true)}>About</button>
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
  )
}
