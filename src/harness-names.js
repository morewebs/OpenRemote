// The product's names for the harnesses the daemon can drive. Harness
// ids are stable; display names are the harnesses' own spellings. Each
// carries its brand mark - the icon its own users know it by (the
// prototype's map, keyed to the daemon's real ids).
import { Asterisk, OpenAiLogo, Pi, Planet, Code } from '@phosphor-icons/react'
import { siClaudecode, siOpencode } from 'simple-icons'

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

// The prototype's icon vocabulary: a brand mark where one exists, the
// harness's own glyph otherwise. Antigravity takes the generic code mark.
export function harnessIcon(id) {
  switch (id) {
    case 'claude':
      return { icon: Asterisk, brand: siClaudecode }
    case 'codex':
      return { icon: OpenAiLogo }
    case 'grok':
      return { icon: Planet }
    case 'pi':
      return { icon: Pi }
    case 'opencode':
      return { icon: Code, brand: siOpencode }
    case 'agy':
      return { icon: Code }
    default:
      return null
  }
}
