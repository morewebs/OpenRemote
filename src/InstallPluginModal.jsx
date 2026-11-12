// Install a plugin: say where it is (a folder, a git repo, an archive) or
// pick it in Discover, read what it is - its runtime, the powers it asks
// for, what it adds, whether it runs outside a sandbox - choose who it is
// for, and only then approve. Nothing is installed until the Install
// button; closing before that leaves nothing behind. A plugin a
// marketplace makes with a command shows the command first: nothing runs
// until the user has seen it. A plugin a project's repo carries, or one
// the account has on its other devices, comes in as `source` and is read
// at once, for this computer's own approval. A plugin written for another
// app's plugin layout also says what the install rewrites for Claude Code
// and what it leaves out, so the review matches what will run.

import { useEffect, useRef, useState } from 'react'
import { X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { canPickFolder, pickFile, pickFolder } from './pick-folder.js'
import {
  CLAUDE_PART_LABEL,
  POWER_LABEL,
  RUNTIME_LABEL,
  SCOPE_HINT,
  SCOPE_LABEL,
  contributionLines,
  defaultScope,
  entrySource,
  installFailedNote,
  sourceRequest,
} from './plugins.js'
import { projectName } from './projects.js'
import { checkSource } from './form-checks.js'
import { errorCopy } from './error-copy.js'
import { useDialog } from './dialog-focus.js'
import './devices.css'
import './plugins.css'

const KINDS = [
  { id: 'folder', label: 'Folder', placeholder: 'The plugin folder on this computer' },
  { id: 'git', label: 'Git repo', placeholder: 'https://github.com/owner/plugin' },
  { id: 'archive', label: 'Archive', placeholder: 'A .zip or .tar.gz - a file or a link' },
]

export default function InstallPluginModal({ onClose, entry = null, source = null }) {
  const { stagePlugin, installStagedPlugin, cancelStagedPlugin, projects } = useConsole()
  const [kind, setKind] = useState('folder')
  const [value, setValue] = useState('')
  // The source's own shape, said under the field once the person has left it:
  // a link is not a folder, a bare word is neither a repo nor an archive. The
  // button refuses the same shapes, so the field and the button agree.
  const sourceProblem = checkSource(kind, value)
  const [sourceTouched, setSourceTouched] = useState(false)
  const [busy, setBusy] = useState(Boolean(entry || source))
  const [staged, setStaged] = useState(null)
  // Who it is for, chosen in the review: a scope, and a project for the
  // two project scopes.
  const [scope, setScope] = useState('machine')
  const [project, setProject] = useState(null)
  const [needsCommand, setNeedsCommand] = useState(null)
  const [error, setError] = useState(null)
  const stagedRef = useRef(null)
  stagedRef.current = staged
  const panelRef = useRef(null)

  // Closing before approval drops the staged copy: nothing is left behind.
  // While a read or an install is in flight the stage belongs to that call,
  // so closing is refused: cancelling it here would pull the copy out from
  // under the install (and the failure would land after the dialog was
  // gone, where nobody could see it). Esc, the X and a backdrop click all
  // come through here, so one guard covers the three.
  const close = () => {
    if (busy) return
    const pending = stagedRef.current
    if (pending) cancelStagedPlugin(pending.stage_id).catch(() => {})
    onClose()
  }

  // Esc is the same close: a staged copy is cancelled on the way out. The rest
  // of the dialog behaviour (focus in, Tab kept inside, the app inert behind,
  // focus back on the opener) comes from the family's shared plumbing.
  useDialog(panelRef, close)

  const read = async (source) => {
    if (!source) return
    // One read at a time, for the same reason close() refuses while busy: the
    // stage belongs to the call that started it. A second Enter while the first
    // is out would stage a second copy and leave the first one's id lost, so
    // the guard lives here, where the key path and the button both arrive,
    // rather than only on the button.
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      const reply = await stagePlugin(source)
      if (reply.result?.needs_command) {
        setNeedsCommand(reply.result.needs_command)
      } else {
        setNeedsCommand(null)
        setStaged(reply.result)
        setScope(defaultScope(reply.result))
        setProject(reply.result?.for_project ?? reply.result?.installed_project ?? projects?.[0]?.id ?? null)
      }
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusy(false)
    }
  }

  // From Discover, a project's repo or the account: read it straight away.
  useEffect(() => {
    if (entry) read(entrySource(entry))
    else if (source) read(source)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const forProject = scope === 'project' || scope === 'project_local'

  const install = async () => {
    setBusy(true)
    setError(null)
    try {
      await installStagedPlugin(staged.stage_id, { scope, project: forProject ? project : undefined })
      setStaged(null)
      onClose()
    } catch (err) {
      // The daemon takes the staged copy when the install starts, so a
      // failed attempt leaves nothing that could install again: the
      // review closes and the screen behind it reads the source once
      // more. Its value is still in state, so that is one press.
      setStaged(null)
      setBusy(false)
      setError(installFailedNote(errorCopy(err)))
    }
  }

  const browse = async () => {
    const path = kind === 'folder' ? await pickFolder('Choose the plugin folder') : await pickFile('Choose the plugin archive')
    if (path) setValue(path)
  }

  const manifest = staged?.manifest
  const name = entry?.display_name ?? entry?.name ?? source?.name ?? manifest?.name
  const adds = contributionLines(manifest?.contributes)
  const scopes = staged?.scopes ?? ['machine']
  const updating = staged?.replaces != null
  const newPowers = staged?.new_powers ?? []
  const fromMarketplace = staged && staged.marketplace !== 'local'
  const title = manifest
    ? `${updating ? 'Update' : 'Install'} ${manifest.name}?`
    : needsCommand
      ? 'Run this command?'
      : entry || source
        ? `Install ${name ?? 'the plugin'}`
        : 'Install a plugin'

  // What the X says while the guard refuses it. A disabled control with a
  // plain "Close" title is the dead UI the console promised not to ship: it
  // looks live, does nothing, and never says why.
  const closeTitle = busy
    ? 'Wait for this step to finish or fail - closing now would cancel it'
    : 'Close'

  return (
    <div
      className="dv-modal-backdrop"
      onMouseDown={(e) => {
        // Deliberate no-op while busy, not a dead handler: the backdrop comes
        // through the same close() as the X and Esc, and that one refuses to
        // abandon a read or an install. The note under the title and the
        // disabled X are what tell the user why the three paths are inert.
        if (e.target === e.currentTarget) close()
      }}
    >
      <div
        className="dv-modal dv-modal--form"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-busy={busy || undefined}
        aria-label={title}
      >
        <div className="dv-modal-head">
          <h2 className="dv-modal-title">{title}</h2>
          <button
            className="dv-modal-close"
            onClick={close}
            disabled={busy}
            title={closeTitle}
            aria-label={closeTitle}
          >
            <X size={14} weight="bold" />
          </button>
        </div>

        {busy && (
          <p className="pg-note" role="status">
            This step has to finish or fail before the dialog can close: cancelling it would pull the
            copy out from under it.
          </p>
        )}

        {manifest ? (
          <>
            <p className="dv-modal-hint">
              {manifest.version ? `Version ${manifest.version}. ` : ''}
              {manifest.description ?? ''}
            </p>
            {updating && (
              <p className="pg-note">
                This replaces the installed {staged.id}
                {staged.replaces ? ` ${staged.replaces}` : ''}.
              </p>
            )}
            {newPowers.length > 0 && (
              <p className="pg-warn-block">
                This version asks for more than the one you have: {newPowers.map((w) => POWER_LABEL[w] ?? w).join(', ').toLowerCase()}.
              </p>
            )}
            <div className="pg-review">
              {fromMarketplace && (
                <div className="pg-review-row">
                  <span className="pg-review-label">From</span>
                  <span>{staged.marketplace}</span>
                </div>
              )}
              <div className="pg-review-row">
                <span className="pg-review-label">Runs as</span>
                <span>
                  {RUNTIME_LABEL[manifest.runtime] ?? manifest.runtime}
                  {manifest.entry && <code className="pg-command pg-review-sub pv">{manifest.entry}</code>}
                </span>
              </div>
              {adds.length > 0 && (
                <div className="pg-review-row">
                  <span className="pg-review-label">Adds</span>
                  <ul className="pg-powers">
                    {adds.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                </div>
              )}
              {manifest.claude_parts.length > 0 && (
                <div className="pg-review-row">
                  <span className="pg-review-label">Carries</span>
                  <span>{manifest.claude_parts.map((p) => CLAUDE_PART_LABEL[p] ?? p).join(', ')}</span>
                </div>
              )}
              <div className="pg-review-row">
                <span className="pg-review-label">It may</span>
                {manifest.powers.length ? (
                  <ul className="pg-powers">
                    {manifest.powers.map((w) => (
                      <li key={w}>
                        {POWER_LABEL[w] ?? w}
                        {newPowers.includes(w) && <span className="pg-new">new</span>}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <span>Nothing beyond itself</span>
                )}
              </div>
              {manifest.adapted?.length > 0 && (
                <div className="pg-review-row">
                  <span className="pg-review-label">Adapted</span>
                  <span>
                    <span className="pg-review-lead">Written for another app&apos;s plugin layout.</span>
                    <ul className="pg-powers">
                      {manifest.adapted.map((note) => (
                        <li key={note}>{note}.</li>
                      ))}
                    </ul>
                  </span>
                </div>
              )}
              {manifest.left_out?.length > 0 && (
                <div className="pg-review-row">
                  <span className="pg-review-label">Left out</span>
                  <ul className="pg-powers">
                    {manifest.left_out.map((note) => (
                      <li key={note}>{note}.</li>
                    ))}
                  </ul>
                </div>
              )}
              {staged.command && (
                <div className="pg-review-row">
                  <span className="pg-review-label">Made by</span>
                  <span>
                    <code className="pg-command pv">{staged.command.command}</code>
                    <span className="pg-review-sub">Runs again each time a Claude chat starts.</span>
                  </span>
                </div>
              )}
            </div>
            <div className="pg-review-row pg-scope">
              <span className="pg-review-label">For</span>
              <div>
                <div className="pn-pick" role="radiogroup" aria-label="Who it is for">
                  {scopes.map((s) => (
                    <button
                      key={s}
                      type="button"
                      role="radio"
                      aria-checked={s === scope}
                      className={s === scope ? 'on' : ''}
                      onClick={() => setScope(s)}
                    >
                      <span className="pn-pick-name">{SCOPE_LABEL[s] ?? s}</span>
                    </button>
                  ))}
                </div>
                <span className="pg-review-sub">{SCOPE_HINT[scope]}</span>
                {forProject &&
                  ((projects ?? []).length ? (
                    <div className="pn-pick pg-scope-projects" role="radiogroup" aria-label="Which project">
                      {projects.map((p) => (
                        <button
                          key={p.id}
                          type="button"
                          role="radio"
                          aria-checked={p.id === project}
                          className={p.id === project ? 'on' : ''}
                          onClick={() => setProject(p.id)}
                        >
                          <span className="pn-pick-name">{projectName(p)}</span>
                        </button>
                      ))}
                    </div>
                  ) : (
                    <span className="pg-review-sub">Add a project first: there is none on this computer yet.</span>
                  ))}
              </div>
            </div>
            {manifest.not_sandboxed && (
              <p className="pg-warn-block">
                Runs as you, not sandboxed. It runs programs with your account&apos;s rights, and no
                operating system lets OpenRemote fence that in. Install it only if you trust where
                it came from.
              </p>
            )}
            {error && <p className="pn-error">{error}</p>}
            <div className="pg-review-actions">
              <button type="button" className="pn-btn" onClick={close} disabled={busy}>
                Cancel
              </button>
              <button type="button" className="dv-act" onClick={install} disabled={busy || (forProject && !project)}>
                {busy ? (updating ? 'Updating…' : 'Installing…') : updating ? 'Update' : 'Install'}
              </button>
            </div>
          </>
        ) : needsCommand ? (
          <>
            <p className="dv-modal-hint">
              {name} is made by running this command on this computer. It runs as you, not
              sandboxed, and once installed it runs again each time a Claude chat starts.
            </p>
            <code className="pg-command pg-command--block pv">{needsCommand.command}</code>
            {error && <p className="pn-error">{error}</p>}
            <div className="pg-review-actions">
              <button type="button" className="pn-btn" onClick={close} disabled={busy}>
                Cancel
              </button>
              <button
                type="button"
                className="dv-act"
                disabled={busy}
                onClick={() => read(entrySource(entry, needsCommand.command))}
              >
                {busy ? 'Running…' : 'Run it and read the plugin'}
              </button>
            </div>
          </>
        ) : entry || source ? (
          <>
            <p className="dv-modal-hint">
              {busy ? `Fetching ${name} to read what it asks for…` : 'Nothing is installed until you have seen what it asks for.'}
            </p>
            {error && <p className="pn-error">{error}</p>}
            {!busy && (
              <div className="pg-review-actions">
                <button type="button" className="pn-btn" onClick={close}>
                  Close
                </button>
                <button type="button" className="dv-act" onClick={() => read(entry ? entrySource(entry) : source)}>
                  Try again
                </button>
              </div>
            )}
          </>
        ) : (
          <>
            <p className="dv-modal-hint">Nothing is installed until you have seen what it asks for.</p>
            <div className="pn-pick">
              {KINDS.map((k) => (
                <button
                  key={k.id}
                  type="button"
                  className={k.id === kind ? 'on' : ''}
                  onClick={() => {
                    setKind(k.id)
                    setError(null)
                  }}
                >
                  <span className="pn-pick-name">{k.label}</span>
                </button>
              ))}
            </div>
            <label className="dv-host">
              {KINDS.find((k) => k.id === kind).label}
              <span className="pg-source-row">
                <input
                  className="dv-command"
                  value={value}
                  onChange={(e) => setValue(e.target.value)}
                  onBlur={() => setSourceTouched(true)}
                  onKeyDown={(e) => {
                    // Enter is the button beside the field: it obeys the same
                    // shape check, or the sentence under the field is a lie.
                    if (e.key === 'Enter' && !sourceProblem) read(sourceRequest(kind, value))
                  }}
                  placeholder={KINDS.find((k) => k.id === kind).placeholder}
                  spellCheck={false}
                  autoComplete="off"
                  aria-invalid={sourceTouched && sourceProblem ? 'true' : undefined}
                  aria-describedby={sourceTouched && sourceProblem ? 'pg-source-problem' : undefined}
                />
                {canPickFolder && kind !== 'git' && (
                  <button type="button" className="pn-btn" onClick={browse}>
                    Browse
                  </button>
                )}
              </span>
            </label>
            {/* A shape the console can judge on its own, said once the person
                has left the field: the daemon still decides whether the thing
                exists, so only a shape that certainly cannot work is refused. */}
            {sourceTouched && sourceProblem && (
              <p className="pn-note" id="pg-source-problem" role="alert">
                {sourceProblem}
              </p>
            )}
            {error && <p className="pn-error">{error}</p>}
            <button
              type="button"
              className="dv-act dv-connect"
              disabled={!sourceRequest(kind, value) || Boolean(sourceProblem) || busy}
              onClick={() => {
                if (sourceProblem) return
                read(sourceRequest(kind, value))
              }}
            >
              {busy ? 'Reading…' : 'Read the plugin'}
            </button>
          </>
        )}
      </div>
    </div>
  )
}
