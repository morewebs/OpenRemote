import { useEffect, useState } from 'react'
import AboutModal from './AboutModal.jsx'
import SignInModal from './SignInModal.jsx'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { signedIn } from './cloud.js'
import { harnessName } from './harness-names.js'
import { SIGNIN_HARNESSES } from './harness-manager.js'
import { loadDefaults, saveDefaults } from './defaults.js'
import { modelDisplayName } from './model-display.js'
import {
  loadPrefs,
  savePrefs,
  applyReduceMotion,
  applyDensity,
} from './settings.js'
import { hasTauri } from './pick-folder.js'
import { version } from '../package.json'
import './devices.css'
import './panels.css'
import './settings.css'

// The harnesses whose own login command the daemon can relay - the
// Sign-in action renders only there (no dead UI elsewhere).

// The rail's contract: one anchor per section, in panel order. The ids
// match the section elements, so the rail stays honest as sections
// change - add a section, add its anchor here.
const SECTIONS = [
  { id: 'new-tasks', label: 'New tasks' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'startup', label: 'Startup' },
  { id: 'cloud', label: 'Cloud' },
  { id: 'daemon', label: 'Daemon' },
  { id: 'harnesses', label: 'Harnesses' },
  { id: 'this-install', label: 'This install' },
]

export default function SettingsView({ onReplay }) {
  const {
    connection,
    capabilities,
    sessions,
    disconnect,
    modelsFor,
    cloud,
    signOutCloud,
    projects: projectsList,
    removeProject,
  } = useConsole()
  const [confirmSignOut, setConfirmSignOut] = useState(false)
  const inCloud = signedIn(cloud)
  const harnessList = capabilities?.harnesses ?? []
  const available = (harnessList ?? []).filter((h) => h.available)
  const [defaults, setDefaults] = useState(() => loadDefaults())
  const [models, setModels] = useState([])
  const [picker, setPicker] = useState(null)
  const [about, setAbout] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [prefs, setPrefs] = useState(() => loadPrefs())
  const [confirmDisconnect, setConfirmDisconnect] = useState(false)
  const defaultHarness = available.find((h) => h.id === defaults.harness) ?? available[0] ?? null

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

  // The effort row offers only the default model's own tiers - the same
  // rule the composer holds. A default model without tiers, or a saved
  // tier the model doesn't offer, means no row: never a dead selection.
  const defaultModel =
    models.find((m) => m.model === defaults.model) ??
    models.find((m) => m.is_default) ??
    null
  const effortTiers = defaultModel?.reasoning_efforts ?? []
  const savedEffort = defaults.efforts?.[defaultHarness?.id] ?? null
  const chosenEffort = effortTiers.includes(savedEffort) ? savedEffort : null

  const setPref = (partial) => {
    savePrefs(partial)
    setPrefs(loadPrefs())
  }

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const toggleReduce = () => {
    const value = !prefs.reduce_motion
    setPref({ reduce_motion: value })
    applyReduceMotion(document, value)
  }

  // The Windows startup toggle: the shell's autostart plugin writes the
  // Run key. The fact is asked once on open; toggling flips it through
  // the plugin and re-reads. Browser dev has no shell - the row never
  // renders there.
  const [autoStart, setAutoStart] = useState(null)
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    ;(async () => {
      try {
        const { isEnabled } = await import('@tauri-apps/plugin-autostart')
        const enabled = await isEnabled()
        if (!cancelled) setAutoStart(enabled)
      } catch {
        /* the row stays absent */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])
  const toggleAutoStart = async () => {
    if (autoStart == null) return
    try {
      const { isEnabled, enable, disable } = await import('@tauri-apps/plugin-autostart')
      if (autoStart) await disable()
      else await enable()
      setAutoStart(await isEnabled())
    } catch {
      /* the fact stays what it was */
    }
  }

  // Keeping OpenRemote in the tray when the window closes: the shell's own
  // setting (on by default when this computer is a machine). Absent in the
  // browser, and where no tray can be shown.
  const [tray, setTray] = useState(null)
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    ;(async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const view = await invoke('tray_prefs')
        if (!cancelled) setTray(view)
      } catch {
        /* the row stays absent */
      }
    })()
    return () => {
      cancelled = true
    }
  }, [])
  const toggleTray = async () => {
    if (!tray) return
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      setTray(await invoke('set_tray_prefs', { keepRunning: !tray.keep_running }))
    } catch {
      /* the fact stays what it was */
    }
  }

  // The shell restart: this side is a single invoke; the console heals
  const [restarting, setRestarting] = useState(false)
  const restartDaemon = async () => {
    if (!hasTauri || restarting) return
    setRestarting(true)
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('daemon_restart')
    } catch {
      /* the poll reports the outcome either way */
    }
    setTimeout(() => setRestarting(false), 4000)
  }

  // The daemon's port and data dir, straight from the shell's own
  // record of the sidecar. Browser dev has no sidecar - no dead rows.
  const [daemonDetail, setDaemonDetail] = useState(null)
  useEffect(() => {
    if (!hasTauri) return
    let cancelled = false
    const ask = async () => {
      try {
        const { invoke } = await import('@tauri-apps/api/core')
        const info = await invoke('daemon_info')
        if (!cancelled && info?.url) setDaemonDetail(info)
      } catch {
        /* the row simply stays absent */
      }
    }
    ask()
    return () => {
      cancelled = true
    }
  }, [connection.state])

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
                ? modelDisplayName(models, defaults.model)
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
        {effortTiers.length > 0 && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Effort</div>
              <div className="pn-detail">{defaultHarness?.name}'s own tier words for {defaultModel?.display_name ?? defaultModel?.model}.</div>
            </div>
            <button
              type="button"
              className="pn-btn"
              onClick={openPicker('effort')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'effort'}
            >
              {chosenEffort ?? `${defaultHarness?.name} default`}
            </button>
            {picker?.kind === 'effort' && (
              <PickerMenu
                label="Effort"
                searchPlaceholder="Search efforts"
                items={effortTiers.map((t) => ({ id: t, name: t }))}
                groups={null}
                selectedId={chosenEffort ?? ''}
                onChoose={(id) =>
                  set({ efforts: { ...defaults.efforts, [defaultHarness.id]: id } })
                }
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
              />
            )}
          </div>
        )}
        <div className="pn-row">
          <div>
            <div className="pn-name">Projects</div>
            <div className="pn-detail">
              {projectsList.length
                ? 'The folders whose chats group under their project. Removing a project keeps every chat.'
                : 'No projects yet. Add one from the home screen.'}
            </div>
          </div>
          <div className="pn-actions">
            {projectsList.map((p) => (
              <button
                key={p.id}
                type="button"
                className="pn-btn"
                title={`Remove the project - its chats stay. Folder: ${p.folders[0]}`}
                onClick={() => removeProject(p.id)}
              >
                {p.folders[0].split(/[\\/]/).filter(Boolean).pop()} ✕
              </button>
            ))}
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
        {autoStart != null && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Launch at login</div>
              <div className="pn-detail">Starts OpenRemote when you sign in to this computer.</div>
            </div>
            <button
              type="button"
              className={autoStart ? 'pn-btn on' : 'pn-btn'}
              onClick={toggleAutoStart}
            >
              {autoStart ? 'On' : 'Off'}
            </button>
          </div>
        )}
        {tray?.available && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Keep running in the tray</div>
              <div className="pn-detail">
                {tray.machine
                  ? 'Closing the window leaves OpenRemote running, so your other devices can still reach this one. Quit from the tray icon.'
                  : 'Closing the window leaves OpenRemote running, so chats here keep going. Quit from the tray icon.'}
              </div>
            </div>
            <button type="button" className={tray.keep_running ? 'pn-btn on' : 'pn-btn'} onClick={toggleTray}>
              {tray.keep_running ? 'On' : 'Off'}
            </button>
          </div>
        )}
      </div>

      <h2 className="pn-section" id="stg-cloud">Cloud</h2>
      <p className="pn-lead">Your moreweb account, and this computer's place in it.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">{inCloud ? `Signed in as ${cloud.account?.email ?? 'your account'}` : 'Not signed in'}</div>
            <div className="pn-detail">
              {inCloud
                ? confirmSignOut
                  ? 'Synced chats stay on this computer; they stop syncing until you sign in again.'
                  : `This computer is ${cloud.device?.name ?? 'here'}, ${cloud.device?.kind === 'machine' ? 'a machine your other devices can run chats on' : 'a desktop'}.`
                : 'Switch to Cloud to sign in. Local chats never need an account.'}
            </div>
          </div>
          {inCloud &&
            (confirmSignOut ? (
              <div className="pn-actions">
                <button
                  type="button"
                  className="pn-btn"
                  onClick={async () => {
                    setConfirmSignOut(false)
                    await signOutCloud()
                  }}
                >
                  Sign out
                </button>
                <button type="button" className="pn-btn" onClick={() => setConfirmSignOut(false)}>Cancel</button>
              </div>
            ) : (
              <button type="button" className="pn-btn" onClick={() => setConfirmSignOut(true)}>
                Sign out
              </button>
            ))}
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
        {daemonDetail?.url && (
          <div className="pn-row">
            <div>
              <div className="pn-name">{daemonDetail.url.replace(/^https?:\/\//, '')}</div>
              <div className="pn-detail">{daemonDetail.data_dir ?? 'The local address the console talks to.'}</div>
            </div>
          </div>
        )}
        {hasTauri && (
          <div className="pn-row">
            <div>
              <div className="pn-name">Restart</div>
              <div className="pn-detail">
                {restarting
                  ? 'The daemon is coming back; the console reconnects on its own.'
                  : 'Stops running chats (stopped ones can resume) and starts a fresh daemon.'}
              </div>
            </div>
            <button
              type="button"
              className="pn-btn"
              onClick={restartDaemon}
              disabled={restarting}
            >
              {restarting ? 'Restarting' : 'Restart'}
            </button>
          </div>
        )}
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
