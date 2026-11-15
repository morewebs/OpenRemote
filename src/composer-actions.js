// The menu names only capabilities backed by an action in this composer.
export function composerActions({ images = true, mobile = false, clipboard = false, browser = false, pluginActions = [], plugins = false } = {}) {
  return [
    ...(images && mobile ? [{ id: 'camera', name: 'Camera', icon: 'camera', group: 'Add', tile: true }] : []),
    ...(images ? [{ id: 'photos', name: 'Photos', icon: 'photos', group: 'Add', tile: true }] : []),
    { id: 'files', name: 'Files', icon: 'files', group: 'Add', tile: true, note: 'PDF, Word, spreadsheets and text up to 10 MB' },
    ...(clipboard ? [{ id: 'clipboard', name: 'Paste from clipboard', icon: 'clipboard', group: 'Context' }] : []),
    { id: 'reference', name: 'Reference files or chats', icon: 'reference', group: 'Context', hint: '@' },
    ...(browser ? [{ id: 'browser', name: 'Browser context', icon: 'browser', group: 'Context', hint: 'submenu' }] : []),
    { id: 'commands', name: 'Commands and skills', icon: 'commands', group: 'Tools', hint: '/' },
    ...pluginActions.map(a => ({ ...a, id: `plugin:${a.id}`, icon: 'plugin', detail: a.pluginName, group: 'Tools' })),
    ...(plugins ? [{ id: 'plugins', name: 'Browse plugins', icon: 'plugin', group: 'Tools', hint: 'submenu' }] : []),
  ]
}

export function beginCompletion(text, start, end, marker) {
  const from = Math.max(0, Math.min(text.length, start ?? text.length))
  const to = Math.max(from, Math.min(text.length, end ?? from))
  const prefix = from > 0 && !/\s/.test(text[from - 1]) ? ' ' : ''
  return { text: text.slice(0, from) + prefix + marker + text.slice(to), caret: from + prefix.length + marker.length }
}
