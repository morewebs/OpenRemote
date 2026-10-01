// The product's names for the harnesses the daemon can drive. Harness
// ids are stable; display names are the harnesses' own spellings.
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
