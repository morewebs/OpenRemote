// Settings > Plugins > <name>: each plugin's own settings, drawn from the
// list its install record holds (the daemon checked it at review) - text, a
// secret, on or off, a number, one of a few choices - each with its label
// and help. A change saves as it is made (text on Enter or leaving the
// field). A secret is write-only: the console never gets one back, so the row
// says only whether it is set, with Replace and Clear (asked twice).

import { useEffect, useState } from 'react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { usePlugins } from './plugins-host.jsx'
import { parseInput, pluginSlug, shownValue } from './plugin-runtime.js'
import { scopeLabel } from './plugins.js'
import { projectName } from './projects.js'
import { errorCopy } from './error-copy.js'

export default function PluginSettings() {
  const host = usePlugins()
  const list = host?.settingsPlugins ?? []
  if (list.length === 0) {
    return (
      <p className="stg-lead">
        No plugin installed here has settings of its own. A plugin that has some lists them here once it is
        installed.
      </p>
    )
  }
  return list.map((record) => <PluginBlock key={record.plugin.id} record={record} host={host} />)
}

function PluginBlock({ record, host }) {
  const { projects } = useConsole()
  const id = record.plugin.id
  const slug = pluginSlug(id)
  const [view, setView] = useState(null)
  const [loadError, setLoadError] = useState(null)
  const [error, setError] = useState(null)
  const { loadSettings, saveSettings } = host
  const project = (projects ?? []).find((p) => p.id === record.plugin.project)

  useEffect(() => {
    let cancelled = false
    setLoadError(null)
    loadSettings(id)
      .then((v) => {
        if (!cancelled) setView(v)
      })
      .catch((err) => {
        if (!cancelled) setLoadError(errorCopy(err))
      })
    return () => {
      cancelled = true
    }
  }, [id, record.plugin.updated_at, loadSettings])

  const save = async (values) => {
    setError(null)
    try {
      setView(await saveSettings(id, values))
      return true
    } catch (err) {
      setError(errorCopy(err))
      return false
    }
  }

  return (
    <div className="ps-block" data-stg={`plugin-${slug}`}>
      <h3 className="ps-head">
        {record.name}
        <span className="ps-id">{scopeLabel(record.plugin, project ? projectName(project) : null)}</span>
        {!record.plugin.enabled && <span className="ps-off">Off</span>}
      </h3>
      {loadError ? (
        <p className="pn-error" role="alert">{loadError}</p>
      ) : (
        view && (
          <div className="pn-list pn-list--tight">
            {record.ui.settings.map((setting) => (
              <SettingRow key={setting.key} slug={slug} setting={setting} view={view} onSave={save} />
            ))}
          </div>
        )
      )}
      {error && <p className="pn-error" role="alert">{error}</p>}
    </div>
  )
}

function SettingRow({ slug, setting, view, onSave }) {
  const control =
    setting.type === 'secret' ? (
      <SecretControl setting={setting} set={view.secrets?.[setting.key] === 'set'} onSave={onSave} />
    ) : setting.type === 'boolean' ? (
      <ToggleControl setting={setting} value={Boolean(shownValue(setting, view.values))} onSave={onSave} />
    ) : setting.type === 'enum' ? (
      <ChoiceControl setting={setting} value={shownValue(setting, view.values)} onSave={onSave} />
    ) : (
      <TextControl setting={setting} value={shownValue(setting, view.values)} onSave={onSave} />
    )
  return (
    <div className="pn-row" data-stg={`plugin-${slug}-${setting.key}`}>
      <div className="ps-text">
        <div className="pn-name">{setting.label}</div>
        {setting.help && <div className="pn-detail">{setting.help}</div>}
      </div>
      {control}
    </div>
  )
}

// Text and numbers: saved on Enter or on leaving the field; Esc puts the
// saved value back.
function TextControl({ setting, value, onSave }) {
  const saved = value == null ? '' : String(value)
  const [draft, setDraft] = useState(saved)
  const [problem, setProblem] = useState(null)
  useEffect(() => {
    setDraft(saved)
    setProblem(null)
  }, [saved])
  const commit = async () => {
    if (draft === saved) return
    const parsed = parseInput(setting, draft)
    if (parsed.error) {
      setProblem(parsed.error)
      return
    }
    setProblem(null)
    if (!(await onSave({ [setting.key]: parsed.value }))) setDraft(saved)
  }
  return (
    <div className="ps-field">
      <input
        className="ps-input"
        type={setting.type === 'number' ? 'number' : 'text'}
        value={draft}
        min={setting.min ?? undefined}
        max={setting.max ?? undefined}
        placeholder={setting.placeholder ?? (setting.default != null && setting.default !== '' ? String(setting.default) : '')}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault()
            commit()
          } else if (e.key === 'Escape' && draft !== saved) {
            // Esc here puts the value back; it doesn't close what holds it.
            e.preventDefault()
            e.stopPropagation()
            setDraft(saved)
            setProblem(null)
          }
        }}
        aria-label={setting.label}
        aria-invalid={problem ? true : undefined}
        spellCheck={false}
        autoComplete="off"
      />
      {problem && <span className="ps-problem">{problem}</span>}
    </div>
  )
}

function ToggleControl({ setting, value, onSave }) {
  return (
    <button
      type="button"
      className={value ? 'pn-btn on' : 'pn-btn'}
      aria-pressed={value}
      aria-label={setting.label}
      onClick={() => onSave({ [setting.key]: !value })}
    >
      {value ? 'On' : 'Off'}
    </button>
  )
}

function ChoiceControl({ setting, value, onSave }) {
  const [menu, setMenu] = useState(null)
  const chosen = setting.options.find((o) => o.value === value)
  return (
    <>
      <button
        type="button"
        className="pn-btn"
        aria-haspopup="listbox"
        aria-expanded={Boolean(menu)}
        onClick={(e) => setMenu({ x: e.clientX, y: e.clientY })}
      >
        {chosen?.label ?? 'Choose'}
      </button>
      {menu && (
        <PickerMenu
          label={setting.label}
          searchPlaceholder={`Search ${setting.label.toLowerCase()}`}
          items={setting.options.map((o) => ({ id: o.value, name: o.label }))}
          groups={null}
          selectedId={value ?? ''}
          onChoose={(id) => {
            setMenu(null)
            onSave({ [setting.key]: id })
          }}
          onClose={() => setMenu(null)}
          anchor={{ left: menu.x, top: menu.y }}
        />
      )}
    </>
  )
}

// A secret: whether it is set, and a way to replace or clear it. What is
// typed goes to the daemon once and is gone from here.
function SecretControl({ setting, set, onSave }) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')
  const [confirmClear, setConfirmClear] = useState(false)
  const close = () => {
    setEditing(false)
    setDraft('')
  }
  const save = async () => {
    if (!draft) return
    if (await onSave({ [setting.key]: draft })) close()
  }
  if (editing) {
    return (
      <div className="ps-secret">
        <input
          className="ps-input"
          type="password"
          value={draft}
          placeholder={set ? 'The new one' : 'Paste it here'}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              save()
            } else if (e.key === 'Escape') {
              e.preventDefault()
              e.stopPropagation()
              close()
            }
          }}
          aria-label={setting.label}
          autoFocus
          spellCheck={false}
          autoComplete="new-password"
        />
        <button type="button" className="pn-btn" disabled={!draft} onClick={save}>
          Save
        </button>
        <button type="button" className="pn-btn" onClick={close}>
          Cancel
        </button>
      </div>
    )
  }
  return (
    <div className="pn-actions ps-secret">
      <span className={set ? 'pn-state pn-state--running' : 'pn-state'}>{set ? 'Set' : 'Not set'}</span>
      <button
        type="button"
        className="pn-btn"
        onClick={() => {
          setConfirmClear(false)
          setEditing(true)
        }}
      >
        {set ? 'Replace' : 'Set'}
      </button>
      {set &&
        (confirmClear ? (
          <button
            type="button"
            className="pn-btn pn-btn--danger"
            onClick={async () => {
              setConfirmClear(false)
              await onSave({ [setting.key]: null })
            }}
          >
            Clear it
          </button>
        ) : (
          <button type="button" className="pn-btn" onClick={() => setConfirmClear(true)}>
            Clear
          </button>
        ))}
    </div>
  )
}
