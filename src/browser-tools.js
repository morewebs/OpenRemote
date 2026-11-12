// OpenRemote's browser tools as a chat's cards and approvals show them:
// "Browser · click", the element as the agent named it, the address, and
// the one argument worth reading before saying yes (what it types, the
// script it runs). Claude and Codex both name them mcp__openremote__browser_*.
// Pure: tested without a DOM.

const PREFIX = 'mcp__openremote__browser_'

// Some tools take the action as an argument (browser_tabs selects or lists
// downloads, browser_navigate goes back, browser_logs reads the network);
// the old one-action names stay so chats from before still read.
const VERBS = {
  tabs: 'tabs',
  logs: 'logs',
  tab_open: 'open tab',
  tab_close: 'close tab',
  tab_select: 'switch tab',
  navigate: 'go to',
  snapshot: 'read page',
  screenshot: 'screenshot',
  console: 'console',
  network: 'network',
  evaluate: 'run script',
  viewport: 'viewport',
  dialog: 'answer dialog',
  upload: 'upload',
  downloads: 'downloads',
}

export function isBrowserTool(name) {
  return typeof name === 'string' && name.startsWith(PREFIX)
}

function words(value) {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

/**
 * How a browser tool call reads: `{ label, description, url, detail,
 * mono }`. `detail` is the argument the user should see (typed text, a
 * key, a script), `mono` when it is code or paths.
 */
export function browserTool(name, input) {
  const verb = name.slice(PREFIX.length)
  const args = input && typeof input === 'object' ? input : {}
  const facts = {
    label: `Browser · ${VERBS[verb] ?? verb.replace(/_/g, ' ')}`,
    description: words(args.element),
    url: words(args.url),
    detail: null,
    mono: false,
  }
  switch (verb) {
    case 'tabs':
      if (words(args.select)) facts.label = `Browser · ${VERBS.tab_select}`
      else if (args.downloads === true) facts.label = `Browser · ${VERBS.downloads}`
      break
    case 'logs':
      if (args.kind === 'console' || args.kind === 'network') facts.label = `Browser · ${args.kind}`
      break
    case 'navigate':
    case 'go':
      if (words(args.direction)) facts.label = `Browser · ${args.direction}`
      break
    case 'type':
      if (typeof args.text === 'string') {
        facts.detail = `"${args.text}"${args.submit ? ', then Enter' : ''}`
      }
      break
    case 'press':
      facts.detail = words(args.key)
      break
    case 'select':
      if (Array.isArray(args.values)) facts.detail = args.values.join(', ')
      break
    case 'evaluate':
      facts.detail = words(args.script)
      facts.mono = true
      break
    case 'upload':
      if (Array.isArray(args.paths)) {
        facts.detail = args.paths.join('\n')
        facts.mono = true
      }
      break
    case 'wait':
      facts.detail = words(args.text)
        ? `until "${args.text}" shows`
        : words(args.gone)
          ? `until "${args.gone}" goes`
          : Number.isFinite(args.ms)
            ? `${args.ms} ms`
            : null
      break
    case 'viewport':
      facts.detail = args.reset
        ? 'the window size'
        : Number.isFinite(args.width) && Number.isFinite(args.height)
          ? `${args.width} × ${args.height}`
          : null
      break
    case 'dialog':
      facts.detail = args.accept ? `accept${words(args.text) ? `: "${args.text}"` : ''}` : 'dismiss'
      break
    case 'screenshot':
      if (args.full_page) facts.detail = 'the whole page'
      break
    case 'scroll':
      if (Number.isFinite(args.dy) && args.dy) facts.detail = `${args.dy > 0 ? 'down' : 'up'} ${Math.abs(args.dy)} px`
      break
    default:
      break
  }
  return facts
}

/** Results that fold by default: a page read as text runs long. */
export function foldsResult(name) {
  return name === `${PREFIX}snapshot`
}
