// The composer's plugin buttons: one for each composer input an enabled
// JavaScript plugin adds (a microphone for dictation, say), in the foot's
// right group beside the send button - a project's plugin only in that
// project's composers. A click goes to the plugin's hidden frame along with
// this composer's handle: the plugin may then write in this composer, and
// only in one the user clicked it in. It never sends. Each button names its
// plugin, so a plugin's button can't pass for one of the app's own. The
// plugin sets the button's state: active (it breathes), busy, or at rest,
// with a short label beside it. With no such plugin nothing renders.

import { useEffect, useId, useRef } from 'react'
import { useConsole } from './state/console.jsx'
import { usePlugins } from './plugins-host.jsx'
import { PluginGlyph } from './plugin-icons.jsx'
import { buttonWords, insertAt } from './plugin-runtime.js'
import { projectFor } from './projects.js'

export default function PluginComposerButtons({ inputRef, setText, composer, workspace = null, disabled = false }) {
  const host = usePlugins()
  const { projects } = useConsole()
  const handle = useId()
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
  useEffect(() => {
    if (!registerComposer) return undefined
    return registerComposer(handle, { insert: (piece) => insertRef.current(piece), attachKey: composer })
  }, [registerComposer, handle, composer])

  if (!host) return null
  const project = projectFor(projects, workspace)?.id ?? null
  const shown = host.inputsFor(project)
  if (shown.length === 0) return null
  return shown.map(({ pluginId, pluginName, input }) => {
    const state = host.inputState(pluginId, input.id, handle, composer)
    const words = buttonWords(input, pluginName, state)
    return (
      <button
        key={`${pluginId}/${input.id}`}
        type="button"
        className="plg-input"
        data-state={state.state}
        data-plugin={pluginId}
        disabled={disabled}
        aria-pressed={state.state === 'active'}
        aria-label={words.label}
        title={words.title}
        onClick={() => host.clickInput(pluginId, input.id, handle)}
      >
        <PluginGlyph name={input.icon} size={16} />
        {state.label && <span className="plg-input-label">{state.label}</span>}
      </button>
    )
  })
}
