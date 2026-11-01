// The product's names for the harnesses the daemon can drive. Harness
// ids are stable; display names are the harnesses' own spellings. Each
// harness's real brand mark lives in brand-marks.jsx.

const NAMES = {
  claude: 'Claude Code',
  codex: 'Codex',
  grok: 'Grok Build',
  pi: 'Pi Agent',
  opencode: 'OpenCode',
  agy: 'Antigravity',
}

export function harnessName(id) {
  return NAMES[id] ?? id
}

const written = (value) => (typeof value === 'string' && value.trim() ? value.trim() : null)

// The name to print for a harness when the console holds more than an id.
// The map above stays first because those six spellings are the product's
// own; a daemon that words one of them differently does not rename it
// here. A harness this build has no name for is one an installed plugin
// provides, and there the daemon's own `name` is the truth - on the
// harness object itself, or in the capabilities list when only the id is
// at hand. With neither, the id prints as it came, which is still
// something a person can read.
export function harnessLabel(harnessOrId, harnesses = null) {
  const id = typeof harnessOrId === 'string' ? harnessOrId : (harnessOrId?.id ?? null)
  if (id != null && Object.hasOwn(NAMES, id)) return NAMES[id]
  const own = written(harnessOrId && typeof harnessOrId === 'object' ? harnessOrId.name : null)
  if (own) return own
  if (id != null && Array.isArray(harnesses)) {
    const named = written(harnesses.find((h) => h && h.id === id)?.name)
    if (named) return named
  }
  return id
}
