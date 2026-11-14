// The composer's plugin buttons: one for each composer input an enabled
// JavaScript plugin adds (a microphone for dictation, say), in the foot's
// right group beside the send button - a project's plugin only in that
// project's composers. A click goes to the plugin's hidden frame along with
// this composer's handle: the plugin may then write in this composer, and
// only in one the user clicked it in. It never sends. Each button names its
// plugin - in its accessible name, its tooltip and a visible badge with the
// plugin's initial - so a plugin's button can't pass for one of the app's
// own. The
// plugin sets the button's state: active (it breathes), busy, or at rest,
// with a short label beside it. With no such plugin nothing renders.

import { useLayoutEffect, useId, useRef } from 'react'
import { useConsole } from './state/console.jsx'
import { usePlugins } from './plugins-host.jsx'
import { PluginGlyph } from './plugin-icons.jsx'
import { buttonWords, insertAt, pluginInitial } from './plugin-runtime.js'
import { projectFor } from './projects.js'

export function usePluginComposerActions({ inputRef, setText, composer, workspace = null, disabled = false, projectId = undefined, deviceId = null }) {
  const host = usePlugins()
  const { projects } = useConsole()
  const project = projectId === undefined ? projectFor(projects, workspace)?.id ?? null : projectId
  const handle = `${useId()}|${composer}|${deviceId ?? ''}|${project ?? ''}`
  // The plugin's text goes in at the cursor, or over the selection; the
  // cursor ends after it, in the field, ready for more typing.
  const insertRef = useRef(null)
  insertRef.current = (piece) => {
    const field = inputRef.current
    const start = field?.selectionStart
    const end = field?.selectionEnd
    let caret = null
    setText((current) => {
      const next = insertAt(current, start, end, piece)
      caret = next.caret
      return next.text
    })
    requestAnimationFrame(() => {
      if (!field || caret == null || field.disabled) return
      field.focus()
      field.setSelectionRange(caret, caret)
    })
  }
  const registerComposer = host?.registerComposer
  useLayoutEffect(() => {
    if (!registerComposer) return undefined
    return registerComposer(handle, { insert: (piece) => insertRef.current(piece), attachKey: composer })
  }, [registerComposer, handle, composer, project, deviceId])

  if (!host) return []
  const shown = host.inputsFor(project)
  return shown.map(({ pluginId, pluginName, input }) => {
    const state = host.inputState(pluginId, input.id, handle)
    const words = buttonWords(input, pluginName, state)
    return { id: `${pluginId}/${input.id}`, name: input.label, pluginId, pluginName, input, state, words, disabled, onChoose: () => host.clickInput(pluginId, input.id, handle) }
  })
}

function Buttons({ actions }) {
  return actions.map(({ id, pluginId, pluginName, input, state, words, disabled, onChoose }) => {
    return (
      <button
        key={id}
        type="button"
        className="plg-input"
        data-state={state.state}
        data-plugin={pluginId}
        disabled={disabled}
        aria-pressed={state.state === 'active'}
        aria-label={words.label}
        title={words.title}
        onClick={onChoose}
      >
        <PluginGlyph name={input.icon} size={16} />
        {/* Whose button it is, on the button itself: a plugin's paperclip
            or microphone never passes for one of the app's own. */}
        <span className="plg-input-badge" aria-hidden="true">
          {pluginInitial(pluginName)}
        </span>
        {state.label && <span className="plg-input-label">{state.label}</span>}
      </button>
    )
  })
}

function OwnedButtons(props) {
  return <Buttons actions={usePluginComposerActions(props)} />
}

export default function PluginComposerButtons({ actions, ...props }) {
  return actions ? <Buttons actions={actions} /> : <OwnedButtons {...props} />
}
