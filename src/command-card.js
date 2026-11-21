// The tool card's shapes: which tool inputs render as what. A file edit
// is a diff (diff.js); a command run is a terminal card - the command in
// mono with its description as the caption, never a JSON dump. Kept
// pure so it's tested without a harness.

/**
 * Whether a tool input is a command run the card can render as a
 * terminal tile: a shell command string, with an optional description
 * beside it. Recognized by shape, not by tool name - every harness's
 * own Bash/shell tool carries this form, whatever it calls itself.
 */
export function isCommandRun(input) {
  if (!input || typeof input !== 'object') return false
  return typeof input.command === 'string' && input.command.length > 0
}

// The shells an agent wraps a command in: the harness runs `powershell.exe
// -Command '...'` or `bash -lc '...'`, and what the person ran is inside.
const SHELL_KIND = new Map([
  ['powershell', 'powershell'],
  ['pwsh', 'powershell'],
  ['bash', 'posix'],
  ['zsh', 'posix'],
  ['sh', 'posix'],
  ['dash', 'posix'],
  ['cmd', 'cmd'],
])

// PowerShell flags that take a value (skipped with it). `-File` and the
// encoded forms are left alone: what they run is not in the line.
const POWERSHELL_VALUE_FLAGS = new Set(['-executionpolicy', '-ep', '-windowstyle', '-workingdirectory', '-wd', '-version', '-inputformat', '-outputformat', '-configurationname'])
const POWERSHELL_OPAQUE_FLAGS = new Set(['-file', '-f', '-encodedcommand', '-ec', '-e'])

function firstToken(text) {
  const m = /^\s*(?:"([^"]*)"|'([^']*)'|(\S+))/.exec(text)
  return m ? { token: m[1] ?? m[2] ?? m[3], rest: text.slice(m[0].length) } : null
}

// One level of surrounding quotes off a command, only when they wrap the
// whole of it and nothing inside could be mistaken for their end. Quotes
// that do not wrap it cleanly give null: the line is not read, not guessed.
function unquote(text, kind) {
  const body = text.trim()
  const q = body[0]
  if (q !== '"' && q !== "'") return body
  if (body.length < 2 || body[body.length - 1] !== q) return null
  const inner = body.slice(1, -1)
  // How a quote of the same kind is written inside: \" in double quotes, ''
  // in PowerShell's single quotes, '\'' in a POSIX shell's.
  const escaped = q === '"' ? '\\"' : kind === 'powershell' ? "''" : "'\\''"
  const plain = inner.split(escaped).join('')
  if (plain.includes(q)) return null
  return inner.split(escaped).join(q)
}

/**
 * The command a person asked for, out of the shell line a harness ran it
 * through: `"C:\\...\\pwsh.exe" -NoProfile -Command "rg -n x"` is `rg -n x`.
 * A line that is not such a wrapper, or one it cannot read with certainty,
 * comes back unchanged. Never invents anything.
 */
export function shellInner(command) {
  if (typeof command !== 'string') return command
  const first = firstToken(command)
  if (!first) return command
  const base = first.token.split(/[\\/]/).pop().toLowerCase().replace(/\.exe$/, '')
  const kind = SHELL_KIND.get(base)
  if (!kind) return command
  let rest = first.rest
  for (;;) {
    const next = /^\s+(\S+)/.exec(rest)
    if (!next) return command
    const flag = next[1]
    const lower = flag.toLowerCase()
    const after = rest.slice(next[0].length)
    if (kind === 'powershell') {
      if (lower === '-command' || lower === '-c') return finish(command, after, kind)
      if (POWERSHELL_OPAQUE_FLAGS.has(lower)) return command
      if (!lower.startsWith('-')) return command
      rest = POWERSHELL_VALUE_FLAGS.has(lower) ? after.replace(/^\s+\S+/, '') : after
    } else if (kind === 'posix') {
      if (/^-[a-z]*c[a-z]*$/.test(flag)) return finish(command, after, kind)
      if (!flag.startsWith('-')) return command
      rest = after
    } else {
      if (lower === '/c' || lower === '/k') return finish(command, after, kind)
      if (!lower.startsWith('/')) return command
      rest = after
    }
  }
}

function finish(original, rest, kind) {
  const inner = unquote(rest, kind)
  return inner ? inner : original
}

/**
 * One line naming a command: the wrapper off, the first line that says
 * anything, whitespace collapsed, clipped with an ellipsis.
 */
export function commandSummary(command, max = 64) {
  const inner = typeof command === 'string' ? shellInner(command) : ''
  const line = inner.split('\n').map((part) => part.trim()).find(Boolean) ?? ''
  const one = line.replace(/\s+/g, ' ')
  return one.length <= max ? one : `${one.slice(0, max - 1).trimEnd()}…`
}
