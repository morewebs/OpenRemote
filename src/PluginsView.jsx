// Plugins: tools a session can call. Each one is written by hand and
// installed on this computer; a key never leaves it.

import { useState } from 'react'
import AddPluginModal from './AddPluginModal.jsx'
import { useConsole } from './state/console.jsx'
import './devices.css'
import './panels.css'

// The state word the prototype ruled: derived, never stored.
function pluginState(p) {
  if (!p.enabled) return 'off'
  if (p.needs_key && !p.has_key) return 'needs-key'
  return 'running'
}

const STATE_LABEL = { running: 'Running', 'needs-key': 'Needs a key', off: 'Off' }

function machineName(machines, id) {
  return (machines ?? []).find((m) => m.machine.id === id)?.machine.name ?? id
}

export default function PluginsView() {
  const {
    machines,
    plugins,
    removePlugin,
    setPluginEnabled,
    acknowledgePluginKey,
  } = useConsole()
  const [adding, setAdding] = useState(false)
  const [error, setError] = useState(null)
  const installed = plugins ?? []
  // One machine is the honest local state — the plural, the hostname, and
  // the install picker return by themselves when remote machines check in.
  const oneMachine = (machines ?? []).length <= 1
  const machineWord = oneMachine ? 'machine' : 'machines'

  const run = async (action, ...args) => {
    setError(null)
    try {
      await action(...args)
    } catch (err) {
      setError(err.message ?? String(err))
    }
  }

  return (
    <div className="panel">
      <header className="dv-head pn-head">
        <div>
          <h1 className="dv-title">Plugins</h1>
          <p className="dv-meta">
            {oneMachine
              ? 'Tools a session can call. Each one is installed on this computer, and a key never leaves it.'
              : 'Tools a session can call. Each one is installed on a machine, and a key never leaves that machine.'}
          </p>
        </div>
        <button type="button" className="dv-act" onClick={() => setAdding(true)}>
          Add plugin
        </button>
      </header>

      <h2 className="pn-section">On your {machineWord}</h2>
      {installed.length === 0 && (
        <p className="pn-empty">Nothing on your {machineWord} yet. Add one.</p>
      )}
      <div className="pn-list pn-list--wide">
        {installed.map((p) => {
          const state = pluginState(p)
          const host = machineName(machines, p.machine)
          return (
            <div key={p.id} className="pn-row pn-row--stack">
              <div className="pn-row-main">
                <div>
                  <div className="pn-name">{p.name}</div>
                  <div className="pn-detail">
                    {p.detail}
                    {oneMachine ? '' : ` · ${host}`}
                  </div>
                </div>
                <div className="pn-actions">
                  <span className={`pn-state pn-state--${state}`}>{STATE_LABEL[state]}</span>
                  {state === 'running' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setPluginEnabled(p.id, false))}>
                      Disable
                    </button>
                  )}
                  {state === 'needs-key' && (
                    <button type="button" className="pn-btn on" onClick={() => run(() => acknowledgePluginKey(p.id))}>
                      Add key
                    </button>
                  )}
                  {state === 'off' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setPluginEnabled(p.id, true))}>
                      Enable
                    </button>
                  )}
                  {state === 'needs-key' && (
                    <button type="button" className="pn-btn" onClick={() => run(() => setPluginEnabled(p.id, false))}>
                      Disable
                    </button>
                  )}
                  <button type="button" className="pn-btn" onClick={() => run(() => removePlugin(p.id))}>
                    Remove
                  </button>
                </div>
              </div>
              <code className="pn-command">{p.command}</code>
              {state === 'needs-key' && (
                <p className="pn-note">
                  The key stays on {oneMachine ? 'this computer' : host}. Adding it here only records that {oneMachine ? 'it has' : 'the machine has'} one.
                </p>
              )}
            </div>
          )
        })}
      </div>
      {error && <p className="pn-error">{error}</p>}
      {adding && <AddPluginModal onClose={() => setAdding(false)} />}
    </div>
  )
}
