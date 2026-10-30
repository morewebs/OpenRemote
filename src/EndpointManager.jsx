// Custom model endpoints: the machine-local records a harness runs
// through instead of its own provider (a self-hosted gateway, a
// relay on this network). The secret never leaves the daemon - the
// rows carry only metadata (`has_key`), and the key field stays empty
// unless the human is setting a new one.

import { useCallback, useEffect, useRef, useState } from 'react'
import { createRemovalLock } from './removal-lock.js'
import { CaretDown, X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { useBack } from './use-back.js'
import { useDialog } from './dialog-focus.js'
import { harnessName } from './harness-names.js'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { useArmedRow } from './use-armed-row.js'
import { formDiffers } from './form-dirty.js'
import { checkBaseUrl, redactedUrl } from './form-checks.js'
import './devices.css'
import './panels.css'
import { errorCopy } from './error-copy.js'

const AUTH_KINDS = [
  { id: 'bearer', name: 'Bearer token' },
  { id: 'api_key', name: 'API key' },
  { id: 'none', name: 'No key' },
]

const emptyForm = {
  id: null,
  name: '',
  baseUrl: '',
  modelsPath: '',
  defaultModel: '',
  authKind: 'bearer',
  key: '',
  enabled: true,
  isDefault: false,
}

function authLabel(kind) {
  return AUTH_KINDS.find((k) => k.id === kind)?.name ?? kind
}

export default function EndpointManager({ harness, deviceId = null, onChanged = null, onClose = null, closeGuard = null, onEndpoints = null }) {
  const { api } = useConsole()
  const [endpoints, setEndpoints] = useState([])
  const [pending, setPending] = useState(true)
  const [form, setForm] = useState(emptyForm)
  const [editing, setEditing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState(null)
  // A list read that failed. The skeleton belongs to `pending` only: without
  // this, a failed read showed the "no endpoints yet" line, which is a lie, and
  // nothing ever asked again.
  const [loadError, setLoadError] = useState(null)
  const [tested, setTested] = useState({}) // id -> { ok, count } | { ok: false, message }
  const [testing, setTesting] = useState(null)
  const [picker, setPicker] = useState(null)
  // Removing an endpoint drops its stored key with it, and nothing in the
  // daemon or the console keeps a copy: one press is not enough for that.
  // Every other action on the pane disarms, and so does the row scrolling out
  // of view or losing focus (use-armed-row.js).
  const [armed, setArmed] = useState(null)
  const armWatch = useArmedRow(armed, setArmed)
  // The row whose removal is in flight. Two presses used to send two deletes
  // over the same row, and the second answered a 404 while the first was still
  // settling the list; the button disables and says what it is doing.
  const [removing, setRemoving] = useState(null)
  // The lock the press mutates before React renders; `removing` is only what
  // the row draws. See src/removal-lock.js.
  const lockRef = useRef(null)
  // The fields as they were when this editor opened, and the table's own
  // close hint: the same dirty rule the rule editor uses (form-dirty.js), so a
  // backdrop click or Esc cannot throw away a pasted key.
  const opened = useRef(null)
  const nameRef = useRef(null)
  const [closeHint, setCloseHint] = useState(false)
  const dirty = formDiffers(form, opened.current)
  const urlProblem = form.baseUrl.trim() ? checkBaseUrl(form.baseUrl) : null

  // The guarded close the modal asks (Esc, backdrop, X) and the one the table's
  // own Cancel button uses. Without a modal (Settings renders this inline) the
  // close is the editor closing itself.
  const closeEditor = () => {
    if (onClose) {
      onClose()
      return
    }
    setEditing(false)
    setForm(emptyForm)
    setError(null)
    setCloseHint(false)
  }

  const requestClose = () => {
    if (dirty) {
      setCloseHint(true)
      return
    }
    closeEditor()
  }

  // Published to the modal's dialog plumbing, so Esc and the backdrop follow
  // the same rule as Cancel's neighbour instead of closing over a typed key.
  if (closeGuard) closeGuard.current = requestClose

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const load = useCallback(async () => {
    setPending(true)
    setLoadError(null)
    try {
      const list = (await api.device(deviceId).modelEndpoints({ harness })) ?? []
      setEndpoints(list)
    } catch (err) {
      // There is no poll here to heal it: this call is the poll. Say what
      // failed, and give the pane a button that asks again.
      setLoadError(errorCopy(err))
    } finally {
      setPending(false)
    }
  }, [api, deviceId, harness])

  useEffect(() => {
    load()
  }, [load])

  // The settings search indexes what this table holds, so the table hands its
  // list up instead of the view reading the same route a second time - one
  // truth, one fetch. The callback lives in a ref so a fresh closure from the
  // parent cannot re-report a list that has not changed.
  const reportRef = useRef(onEndpoints)
  reportRef.current = onEndpoints
  useEffect(() => {
    reportRef.current?.(endpoints)
  }, [endpoints])

  const startAdd = () => {
    const next = { ...emptyForm }
    setForm(next)
    opened.current = { ...next }
    setCloseHint(false)
    setEditing(true)
    setError(null)
    setArmed(clearRow())
    requestAnimationFrame(() => nameRef.current?.focus())
  }

  const startEdit = (entry) => {
    const next = {
      id: entry.id,
      name: entry.name,
      baseUrl: entry.base_url,
      modelsPath: entry.models_path ?? '',
      defaultModel: entry.default_model ?? '',
      authKind: entry.auth_kind,
      key: '',
      enabled: entry.enabled,
      isDefault: entry.is_default,
    }
    setForm(next)
    opened.current = { ...next }
    setCloseHint(false)
    setEditing(true)
    setError(null)
    setArmed(clearRow())
    requestAnimationFrame(() => nameRef.current?.focus())
  }

  const save = async () => {
    if (saving || urlProblem) return
    setSaving(true)
    setError(null)
    try {
      await api.device(deviceId).saveEndpoint({
        ...(form.id ? { id: form.id } : {}),
        harness,
        name: form.name.trim(),
        baseUrl: form.baseUrl.trim(),
        ...(form.modelsPath.trim() ? { modelsPath: form.modelsPath.trim() } : {}),
        ...(form.defaultModel.trim() ? { defaultModel: form.defaultModel.trim() } : {}),
        authKind: form.authKind,
        ...(form.key ? { key: form.key } : {}),
        enabled: form.enabled,
        isDefault: form.isDefault,
      })
      setEditing(false)
      setForm(emptyForm)
      await load()
      onChanged?.()
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setSaving(false)
    }
  }

  const remove = async (id) => {
    // The claim, not `removing`: state inside this handler is the render's
    // state, so a double click would pass a state check twice. The lock is
    // set synchronously in the press.
    if (!lockRef.current) lockRef.current = createRemovalLock()
    if (!lockRef.current.claim(id)) return
    setError(null)
    setArmed(clearRow())
    setRemoving(id)
    try {
      await api.device(deviceId).removeEndpoint(id)
      await load()
      onChanged?.()
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      lockRef.current.release()
      setRemoving(null)
    }
  }

  const setEnabled = async (id, enabled) => {
    setArmed(clearRow())
    try {
      await api.device(deviceId).setEndpointEnabled(id, enabled)
      await load()
      onChanged?.()
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  const makeDefault = async (id) => {
    setArmed(clearRow())
    try {
      await api.device(deviceId).setDefaultEndpoint(harness, id)
      await load()
      onChanged?.()
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  const clearDefault = async () => {
    setArmed(clearRow())
    try {
      await api.device(deviceId).setDefaultEndpoint(harness, null)
      await load()
      onChanged?.()
    } catch (err) {
      setError(errorCopy(err))
    }
  }

  const test = async (id) => {
    setArmed(clearRow())
    setTesting(id)
    try {
      const models = (await api.device(deviceId).endpointModels(id)) ?? []
      setTested((t) => ({ ...t, [id]: { ok: true, count: models.length } }))
    } catch (err) {
      setTested((t) => ({ ...t, [id]: { ok: false, message: errorCopy(err) } }))
    } finally {
      setTesting(null)
    }
  }

  const executable = harness === 'claude'

  return (
    <div>
      {!executable && (
        <p className="pn-note">
          {harnessName(harness)} endpoints can be stored, but chats can't run through them yet -
          only claude executes custom endpoints today.
        </p>
      )}
      {pending ? (
        <span className="nc-skeleton" aria-hidden="true" />
      ) : loadError ? (
        // The list could not be read. Not the empty state: "no endpoints yet"
        // would claim something the pane never learned.
        <div className="dv-error pn-row pn-row--stack" role="alert">
          <div className="pn-row-main">
            <div>
              <div className="pn-name">The endpoint list could not be read</div>
              <div className="pn-detail">{loadError}</div>
            </div>
          </div>
          <div className="pn-actions">
            <button type="button" className="pn-btn" onClick={load}>
              Try again
            </button>
          </div>
        </div>
      ) : (
        <>
          {endpoints.length === 0 && !editing && (
            <p className="pn-empty ep-empty">
              No custom endpoints yet. Chats run through {harnessName(harness)}'s own provider.
            </p>
          )}
          {endpoints.length > 0 && (
          <div className="pn-list pn-list--wide" data-stg={`models-key-${harness}`}>
          {endpoints.map((entry) => (
            <div key={entry.id} className="pn-row pn-row--stack" data-stg={`endpoint-${harness}-${entry.id}`}>
              <div className="pn-row-main">
                <div>
                  {/* The person names this endpoint and points it at their own
                      server, so both halves are theirs: they blur together. */}
                  <div className="pn-name">
                    <span className="pv">{entry.name}</span>
                    {entry.is_default ? ' · default' : ''}
                    {entry.enabled ? '' : ' · off'}
                  </div>
                  <div className="pn-detail">
                    <span className="pv">{redactedUrl(entry.base_url)}</span> · {authLabel(entry.auth_kind)}
                    {entry.auth_kind !== 'none' && (entry.has_key ? ' · key saved' : ' · no key')}
                    {entry.default_model && ` · ${entry.default_model}`}
                  </div>
                  {tested[entry.id] && (
                    <div className="pn-detail">
                      {tested[entry.id].ok
                        ? `Lists ${tested[entry.id].count} model${tested[entry.id].count === 1 ? '' : 's'} today.`
                        : tested[entry.id].message}
                    </div>
                  )}
                </div>
                <div className="pn-actions">
                  <button type="button" className="pn-btn" onClick={() => test(entry.id)} disabled={testing === entry.id}>
                    {testing === entry.id ? 'Testing…' : 'Test'}
                  </button>
                  <button type="button" className="pn-btn" onClick={() => startEdit(entry)}>
                    Edit
                  </button>
                  {!entry.is_default && entry.enabled && (
                    <button type="button" className="pn-btn" onClick={() => makeDefault(entry.id)}>
                      Default
                    </button>
                  )}
                  <button type="button" className="pn-btn" onClick={() => setEnabled(entry.id, !entry.enabled)}>
                    {entry.enabled ? 'Turn off' : 'Turn on'}
                  </button>
                  {rowIsArmed(armed, entry.id) ? (
                    <button
                      type="button"
                      className="pn-btn pn-btn--danger"
                      ref={armWatch.buttonRef}
                      onBlur={armWatch.onBlur}
                      disabled={removing != null}
                      onClick={() => remove(entry.id)}
                      title="Its saved key goes with it, and there is no undo"
                    >
                      {removing === entry.id ? "Removing…" : "Remove it and its key"}
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="pn-btn"
                      disabled={removing != null}
                      onClick={() => setArmed((curr) => armRow(curr, entry.id))}
                    >
                      Remove
                    </button>
                  )}
                </div>
              </div>
            </div>
          ))}
          </div>
          )}
        </>
      )}
      {endpoints.some((e) => e.is_default) && !editing && (
        <p className="pn-note">
          New chats run through the default.{' '}
          <button type="button" className="pn-btn" onClick={clearDefault}>
            Clear default
          </button>
        </p>
      )}
      {!editing ? (
        <button type="button" className="pn-btn ep-add" data-stg="models-endpoint-add" onClick={startAdd}>
          Add endpoint
        </button>
      ) : (
        <div className="pn-list pn-list--tight">
          <label className="dv-host">
            Name
            <input
              ref={nameRef}
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="Office gateway"
              spellCheck={false}
              autoComplete="off"
              // The dialog would otherwise keep the keyboard on its own close
              // button, so the first word typed here would go nowhere.
              autoFocus
            />
          </label>
          <label className="dv-host">
            Base URL
            <input
              value={form.baseUrl}
              onChange={(e) => setForm((f) => ({ ...f, baseUrl: e.target.value }))}
              placeholder="http://localhost:4000/v1"
              spellCheck={false}
              autoComplete="off"
              aria-invalid={urlProblem ? 'true' : undefined}
              aria-describedby={urlProblem ? 'ep-url-problem' : undefined}
            />
          </label>
          {urlProblem && (
            <p className="pn-note" id="ep-url-problem" role="alert">
              {urlProblem}
            </p>
          )}
          <label className="dv-host">
            Model-list path override (optional)
            <input
              value={form.modelsPath}
              onChange={(e) => setForm((f) => ({ ...f, modelsPath: e.target.value }))}
              placeholder="v1/models"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <label className="dv-host">
            Default model (optional - the chat's pick when none is named)
            <input
              value={form.defaultModel}
              onChange={(e) => setForm((f) => ({ ...f, defaultModel: e.target.value }))}
              placeholder="my-model-id"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <label className="dv-host">
            Credential
            <button
              type="button"
              className="dv-select"
              onClick={openPicker('authKind')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'authKind'}
            >
              {authLabel(form.authKind)}
              <CaretDown size={12} />
            </button>
          </label>
          {picker?.kind === 'authKind' && (
            <PickerMenu
              label="Credential"
              searchPlaceholder="Search credential kinds"
              items={AUTH_KINDS.map((k) => ({ id: k.id, name: k.name }))}
              groups={null}
              selectedId={form.authKind}
              onChoose={(id) => {
                setForm((f) => ({ ...f, authKind: id }))
                setPicker(null)
              }}
              onClose={() => setPicker(null)}
              anchor={{ left: picker.x, top: picker.y }}
            />
          )}
          {form.authKind !== 'none' && (
            <label className="dv-host">
              Key{form.id ? ' (leave empty to keep the saved one)' : ''}
              <input
                type="password"
                value={form.key}
                onChange={(e) => setForm((f) => ({ ...f, key: e.target.value }))}
                placeholder={form.id ? 'Unchanged' : 'Endpoint key'}
                spellCheck={false}
                autoComplete="new-password"
              />
            </label>
          )}
          <label className="dv-check">
            <input
              type="checkbox"
              checked={form.enabled}
              onChange={(e) => setForm((f) => ({ ...f, enabled: e.target.checked }))}
            />
            Turned on
          </label>
          <label className="dv-check">
            <input
              type="checkbox"
              checked={form.isDefault}
              onChange={(e) => setForm((f) => ({ ...f, isDefault: e.target.checked }))}
            />
            The {harnessName(harness)} default - new chats run through it
          </label>
          {error && <p className="pn-note">{error}</p>}
          {closeHint && dirty && (
            <p className="pn-note" role="status">
              This endpoint has unsaved changes. Cancel discards them.
            </p>
          )}
          <div className="pn-row">
            <div className="pn-actions">
              <button
                type="button"
                className="pn-btn on"
                onClick={save}
                disabled={saving || !form.name.trim() || !form.baseUrl.trim() || Boolean(urlProblem)}
              >
                {saving ? 'Saving…' : form.id ? 'Save endpoint' : 'Add endpoint'}
              </button>
              <button
                type="button"
                className="pn-btn"
                onClick={() => {
                  closeEditor()
                }}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
      {error && !editing && <p className="pn-note">{error}</p>}
    </div>
  )
}

// The picker footer and Settings open the same manager in a modal: the
// harness's rows, live from the machine that would run the chats. Esc closes
// this dialog and nothing under it - the rule form that opened it keeps what
// was written in it - unless the endpoint editor inside it has unsaved fields,
// in which case the editor's own close hint answers the key instead.
export function EndpointManagerModal({ harness, deviceId = null, where = null, onClose, onChanged = null }) {
  useBack(true, onClose)
  const panelRef = useRef(null)
  // The editor publishes its guarded close here, so Esc, the backdrop and the
  // X all ask the same question the Cancel button's neighbour answers.
  const closeEditor = useRef(onClose)
  useDialog(panelRef, () => closeEditor.current())
  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) closeEditor.current()
      }}
    >
      <div className="dv-modal dv-modal--form" ref={panelRef} tabIndex={-1} role="dialog" aria-modal="true" aria-label={`Models for ${harnessName(harness)}`}>
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">
            Models for {harnessName(harness)}
            {where ? <span className="pv"> · {where}</span> : ''}
          </h2>
          <button className="dv-modal-close" onClick={() => closeEditor.current()} title="Close">
            <X size={14} weight="bold" />
          </button>
        </div>
        <p className="dv-modal-hint">
          A custom endpoint is a model server on this machine or network that speaks an
          OpenAI-compatible models API. New chats run through the default; a chat's own
          endpoint is its own choice.
        </p>
        <EndpointManager harness={harness} deviceId={deviceId} onChanged={onChanged} onClose={onClose} closeGuard={closeEditor} />
      </div>
    </div>
  )
}
