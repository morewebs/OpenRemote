import { useState } from 'react'
import PickerMenu from './PickerMenu.jsx'
import AboutModal from './AboutModal.jsx'
import { APP, MODELS, harnessName } from './catalog.js'
import { PROJECTS, contextWindow, formatTokens } from './world.js'
import './devices.css'
import './panels.css'

export default function Panels({
  world,
  harnesses,
  onDefaults,
  onSignIn,
  onSignOut,
  onReduceMotion,
  onReset,
  onReplay,
  onUpdated,
}) {
  const [picker, setPicker] = useState(null)
  const [confirmReset, setConfirmReset] = useState(false)
  const [about, setAbout] = useState(false)
  const defaults = world.defaults
  const project = PROJECTS.find((p) => p.id === defaults.project) ?? PROJECTS[0]
  const modelList = MODELS[defaults.harness] ?? []
  const model = modelList.find((m) => m.id === defaults.model) ?? modelList[0]
  const machine = world.devices.find((d) => d.id === defaults.deviceId)

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  return (
    <div className="panel">
      <header className="dv-head">
        <h1 className="dv-title">Settings</h1>
        <p className="dv-meta">Defaults for a new task, which harnesses are signed in, and this install.</p>
      </header>

      <h2 className="pn-section">New tasks</h2>
      <p className="pn-lead">Local tasks run on this computer. Cloud tasks use the machine below.</p>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">Project</div>
            <div className="pn-detail">{project.path}</div>
          </div>
          <button type="button" className="pn-btn" onClick={openPicker('project')}>{project.name}</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Harness</div>
            <div className="pn-detail">What a new task starts on.</div>
          </div>
          <button type="button" className="pn-btn" onClick={openPicker('harness')}>{harnessName(defaults.harness)}</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Model</div>
            <div className="pn-detail">{formatTokens(contextWindow(defaults.model))} context</div>
          </div>
          <button type="button" className="pn-btn" onClick={openPicker('model')}>{model?.name ?? defaults.model}</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Cloud machine</div>
            <div className="pn-detail">Used when the sidebar is set to Cloud.</div>
          </div>
          <button type="button" className="pn-btn" onClick={openPicker('device')}>{machine?.name ?? 'Choose'}</button>
        </div>
      </div>
      {picker?.kind === 'project' && (
        <PickerMenu
          label="Project"
          searchPlaceholder="Search projects"
          items={PROJECTS}
          groups={null}
          selectedId={project.id}
          onChoose={(id) => {
            onDefaults({ project: id })
            setPicker(null)
          }}
          onClose={() => setPicker(null)}
          anchor={{ left: picker.x, top: picker.y }}
          renderTrailing={(p) => <span className="nc-item-path">{p.path}</span>}
        />
      )}
      {picker?.kind === 'harness' && (
        <PickerMenu
          label="Harness"
          searchPlaceholder="Search harnesses"
          items={harnesses}
          groups
          selectedId={defaults.harness}
          onChoose={(id) => {
            onDefaults({ harness: id, model: MODELS[id][0].id })
            setPicker(null)
          }}
          onClose={() => setPicker(null)}
          anchor={{ left: picker.x, top: picker.y }}
        />
      )}
      {picker?.kind === 'model' && (
        <PickerMenu
          label="Model"
          searchPlaceholder="Search models"
          items={modelList}
          groups={null}
          selectedId={model?.id}
          onChoose={(id) => {
            onDefaults({ model: id })
            setPicker(null)
          }}
          onClose={() => setPicker(null)}
          anchor={{ left: picker.x, top: picker.y }}
        />
      )}
      {picker?.kind === 'device' && (
        <PickerMenu
          label="Device"
          searchPlaceholder="Search devices"
          items={world.devices}
          groups={null}
          selectedId={defaults.deviceId}
          onChoose={(id) => {
            onDefaults({ deviceId: id })
            setPicker(null)
          }}
          onClose={() => setPicker(null)}
          anchor={{ left: picker.x, top: picker.y }}
        />
      )}

      <h2 className="pn-section">Harnesses</h2>
      <p className="pn-lead">Signing in is separate from installing. Install happens on a machine, from Machines.</p>
      <div className="pn-list pn-list--tight">
        {harnesses.map((h) => {
          const signed = world.signedIn.includes(h.id)
          return (
            <div key={h.id} className="pn-row">
              <div>
                <div className="pn-name">{h.name}</div>
                <div className="pn-detail">{signed ? 'Signed in' : 'Not signed in'}</div>
              </div>
              <button
                type="button"
                className={signed ? 'pn-btn on' : 'pn-btn'}
                onClick={() => (signed ? onSignOut(h.id) : onSignIn(h.id))}
              >
                {signed ? 'Sign out' : 'Sign in'}
              </button>
            </div>
          )
        })}
      </div>

      <h2 className="pn-section">This install</h2>
      <div className="pn-list pn-list--tight">
        <div className="pn-row">
          <div>
            <div className="pn-name">OpenRemote {world.appVersion}</div>
            <div className="pn-detail">
              {APP.channel} channel
              {world.appVersion !== APP.latest ? ` · ${APP.latest} is the newer build` : ''}
            </div>
          </div>
          <button type="button" className="pn-btn" onClick={() => setAbout(true)}>About</button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Reduce motion</div>
            <div className="pn-detail">Stops the uptime pulse and the update animation.</div>
          </div>
          <button
            type="button"
            className={world.reduceMotion ? 'pn-btn on' : 'pn-btn'}
            onClick={() => onReduceMotion(!world.reduceMotion)}
          >
            {world.reduceMotion ? 'On' : 'Off'}
          </button>
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">Sample workspace</div>
            <div className="pn-detail">Puts machines, sessions, plugins, and rules back to the sample.</div>
          </div>
          {confirmReset ? (
            <div className="pn-actions">
              <button type="button" className="pn-btn" onClick={onReset}>Reset sample</button>
              <button type="button" className="pn-btn" onClick={() => setConfirmReset(false)}>Cancel</button>
            </div>
          ) : (
            <button type="button" className="pn-btn" onClick={() => setConfirmReset(true)}>Reset</button>
          )}
        </div>
        <div className="pn-row">
          <div>
            <div className="pn-name">First-run steps</div>
            <div className="pn-detail">Shows them again. The workspace stays.</div>
          </div>
          <button type="button" className="pn-btn" onClick={onReplay}>Show</button>
        </div>
      </div>
      {about && (
        <AboutModal
          version={world.appVersion}
          latest={APP.latest}
          reduceMotion={world.reduceMotion}
          onUpdated={onUpdated}
          onClose={() => setAbout(false)}
        />
      )}
    </div>
  )
}
