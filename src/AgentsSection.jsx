// Settings > Agents: what a chat's agent may do with other chats. An agent
// can start workers - on any installed harness - and wait for their
// reports; these are the user's limits, kept by the daemon on this
// computer and enforced there, so no agent can talk past them.

import { useEffect, useState } from 'react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { errorCopy } from './error-copy.js'
import { harnessName } from './harness-names.js'
import { pickable } from './harness-manager.js'
import { permissionWords } from './harness-carry.js'
import './devices.css'
import './panels.css'
import './team.css'

const DEFAULT_WORD = '__default'

// A harness's permission words as picker rows: the daemon sends `{ w, l }`
// (its own flag and the label to show).
const wordRows = (modes) =>
  (modes ?? []).map((m) => (typeof m === 'string' ? { id: m, name: m } : { id: m.w, name: m.l || m.w }))

const levels = (n) => `${n} level${n === 1 ? '' : 's'}`
const agents = (n) => `${n} agent${n === 1 ? '' : 's'}`

export default function AgentsSection() {
  const { api, capabilities } = useConsole()
  const [settings, setSettings] = useState(null)
  const [error, setError] = useState(null)
  const [picker, setPicker] = useState(null)
  const installed = (capabilities?.harnesses ?? []).filter(pickable)

  useEffect(() => {
    let cancelled = false
    api
      .agentSettings()
      .then((value) => {
        if (!cancelled) setSettings(value)
      })
      .catch((err) => {
        if (!cancelled) setError(errorCopy(err))
      })
    return () => {
      cancelled = true
    }
  }, [api])

  const change = async (patch) => {
    setError(null)
    setPicker(null)
    try {
      setSettings(await api.setAgentSettings(patch))
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  if (!settings) {
    return (
      <>
        <p className="pn-lead">What a chat's agent may do with other chats.</p>
        {error ? <p className="pn-error">{error}</p> : <p className="pn-empty">Reading the settings...</p>}
      </>
    )
  }

  // None picked means every installed harness.
  const allowed = settings.harnesses.length
    ? settings.harnesses
    : installed.map((h) => h.id)
  const toggleHarness = (id) => {
    const next = allowed.includes(id) ? allowed.filter((x) => x !== id) : [...allowed, id]
    if (next.length === 0) return
    change({ harnesses: next.length === installed.length ? [] : next })
  }

  const numberRow = (kind, label, detail, value, words, max) => (
    <div className="pn-row" key={kind}>
      <div>
        <div className="pn-name">{label}</div>
        <div className="pn-detail">{detail}</div>
      </div>
      <button
        type="button"
        className="pn-btn"
        onClick={openPicker(kind)}
        aria-haspopup="listbox"
        aria-expanded={picker?.kind === kind}
      >
        {words(value)}
      </button>
      {picker?.kind === kind && (
        <PickerMenu
          label={label}
          searchPlaceholder="Search"
          items={Array.from({ length: max }, (_, i) => ({ id: String(i + 1), name: words(i + 1) }))}
          groups={null}
          selectedId={String(value)}
          onChoose={(id) => change({ [kind]: Number(id) })}
          onClose={() => setPicker(null)}
          anchor={{ left: picker.x, top: picker.y }}
        />
      )}
    </div>
  )

  const toggleRow = (key, label, detail) => (
    <div className="pn-row" key={key}>
      <div>
        <div className="pn-name">{label}</div>
        <div className="pn-detail">{detail}</div>
      </div>
      <button
        type="button"
        className={settings[key] ? 'pn-btn on' : 'pn-btn'}
        onClick={() => change({ [key]: !settings[key] })}
      >
        {settings[key] ? 'On' : 'Off'}
      </button>
    </div>
  )

  return (
    <>
      <p className="pn-lead">
        A chat's agent can start other chats as workers - on any harness installed here - and wait
        for their reports. Every worker is a chat in the sidebar: you can open, steer, stop or take
        it over. These limits are kept by the daemon on this computer.
      </p>
      <div className="pn-list pn-list--tight">
        {toggleRow(
          'tools',
          'Team tools',
          'Give agents the tools to start workers and wait for them. Off, no chat is offered them (a chat already running keeps its list until it restarts, but its calls are refused).',
        )}
        {settings.tools && (
          <>
            {numberRow(
              'max_depth',
              'How deep a team goes',
              'Your own chat is the top and its workers are one level down. An agent at the last level can report up but not start more.',
              settings.max_depth,
              levels,
              4,
            )}
            {numberRow(
              'max_children',
              'Workers per agent',
              'How many workers one agent may have running at once. It is told to stop one or reuse an idle one when it asks for more.',
              settings.max_children,
              agents,
              8,
            )}
            {numberRow(
              'max_live',
              'Running in one team',
              'The most agents running at once under one chat, workers of workers included.',
              settings.max_live,
              agents,
              16,
            )}
            {toggleRow(
              'stop_with_lead',
              'Stop workers with their lead',
              'Stopping a chat also stops the workers still running under it.',
            )}
            {toggleRow(
              'sideways',
              'Workers can message each other',
              'Off: a worker reports to its lead and no one else. On: workers under the same lead may send each other messages.',
            )}
            {installed.length > 0 && (
              <div className="pn-row">
                <div>
                  <div className="pn-name">Harnesses workers may use</div>
                  <div className="pn-detail">
                    Any installed one by default; leave on only the ones you want agents to pick from.
                  </div>
                </div>
                <div className="stg-agents-harnesses">
                  {installed.map((h) => (
                    <button
                      key={h.id}
                      type="button"
                      className={allowed.includes(h.id) ? 'pn-btn on' : 'pn-btn'}
                      onClick={() => toggleHarness(h.id)}
                      aria-pressed={allowed.includes(h.id)}
                    >
                      {harnessName(h.id)}
                    </button>
                  ))}
                </div>
              </div>
            )}
            {installed
              .filter((h) => (h.permission_modes ?? []).length > 0)
              .map((h) => {
                const kind = `permission:${h.id}`
                const chosen = settings.permissions?.[h.id]
                const word = permissionWords(h.permission_modes).includes(chosen) ? chosen : null
                const wordLabel = wordRows(h.permission_modes).find((row) => row.id === word)?.name ?? word
                return (
                  <div className="pn-row" key={kind}>
                    <div>
                      <div className="pn-name">Worker permission - {harnessName(h.id)}</div>
                      <div className="pn-detail">
                        What a {harnessName(h.id)} worker may do without asking, in its own words. An
                        agent can't give a worker another one; a worker that needs a decision waits
                        in its own chat for you.
                      </div>
                    </div>
                    <button
                      type="button"
                      className="pn-btn"
                      onClick={openPicker(kind)}
                      aria-haspopup="listbox"
                      aria-expanded={picker?.kind === kind}
                    >
                      {wordLabel ?? `${harnessName(h.id)} default`}
                    </button>
                    {picker?.kind === kind && (
                      <PickerMenu
                        label={`Worker permission - ${harnessName(h.id)}`}
                        searchPlaceholder="Search permission words"
                        items={[
                          { id: DEFAULT_WORD, name: `${harnessName(h.id)} default` },
                          ...wordRows(h.permission_modes),
                        ]}
                        groups={null}
                        selectedId={word ?? DEFAULT_WORD}
                        onChoose={(id) => {
                          const next = { ...settings.permissions }
                          if (id === DEFAULT_WORD) delete next[h.id]
                          else next[h.id] = id
                          change({ permissions: next })
                        }}
                        onClose={() => setPicker(null)}
                        anchor={{ left: picker.x, top: picker.y }}
                        renderSubline={(item) =>
                          item.id === h.default_permission ? (
                            <span className="nc-item-sub">{harnessName(h.id)} runs this by default</span>
                          ) : null
                        }
                      />
                    )}
                  </div>
                )
              })}
          </>
        )}
      </div>
      {error && <p className="pn-error">{error}</p>}
    </>
  )
}
