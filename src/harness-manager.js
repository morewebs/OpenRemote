// The harness manager's row model. Capabilities say what the daemon
// found on this machine; the machine's install specs say what it can
// install, each through its owner's own installer - every harness, on
// every desktop platform the owners ship for. The install command stays
// the daemon's business: a missing row is its name, its status, and an
// Install button.

import { harnessLabel } from './harness-names.js'

// The harnesses whose own login command the daemon can relay - the rest
// sign in through their own setup.
export const SIGNIN_HARNESSES = new Set(['claude', 'codex', 'grok'])

// A harness a picker offers: installed, and its built-in plugin on
// (Plugins). One turned off keeps its chats; it just isn't offered.
export const pickable = (h) => Boolean(h.available) && h.enabled !== false

// The control words the Apps row states, in the console's own vocabulary.
const HONORS = [
  ['effort', 'Effort'],
  ['fast', 'Fast'],
  ['mcp', 'MCP'],
  ['plugins', 'Plugins'],
  ['tools', 'Browser tools'],
  ['attachments', 'Attachments'],
]

/**
 * What one harness honors, as words the manager prints. Null when the daemon
 * did not send the object (an older build): unknown is not "none", and a row
 * must not call a capability off on no evidence.
 */
export function capabilityWords(row) {
  const honors = row?.honors
  if (!honors || typeof honors !== 'object') return null
  return HONORS.map(([key, label]) => ({ key, label, on: honors[key] === true }))
}

// One row per harness the daemon knows, always: the manager is the
// inventory of all six, not just the installed ones. `installable` is
// null while the machines list hasn't answered yet - a missing row then
// stays quiet rather than claiming anything for the moment.
export function harnessRows(capabilities, installable) {
  const specById =
    installable == null ? null : new Map(installable.map((spec) => [spec.harness_id, spec]))
  return (capabilities?.harnesses ?? []).map((h) => {
    const name = harnessLabel(h)
    // The daemon's own reason a harness cannot be used - its CLI is not
    // found, or a plugin speaks a harness API this build does not. The row
    // shows that sentence where it would otherwise only say something is
    // missing, which would leave the person guessing what.
    const problem = typeof h.problem === 'string' && h.problem.trim() ? h.problem.trim() : null
    if (h.available) {
      // The daemon's own probe words: the version it read from the CLI
      // and the path it resolved - technical content, mono in the UI.
      const probe = [h.version, h.path].filter(Boolean).join(' · ') || null
      const signedOut = h.signed_in === false
      const relayable = signedOut && SIGNIN_HARNESSES.has(h.id)
      return {
        id: h.id,
        name,
        statusLabel: signedOut ? 'Not signed in' : 'Ready',
        ready: !signedOut,
        // A harness the daemon cannot sign in for says how its own sign-in works,
        // in the note slot the missing rows use: the row states a fault, and it used
        // to offer nothing at all beside it.
        detail: signedOut && !relayable ? `${name} signs in through its own setup, in a terminal.` : probe,
        detailKind: signedOut && !relayable ? 'note' : 'mono',
        action: relayable ? 'signin' : 'none',
        caps: capabilityWords(h),
      }
    }
    const spec = specById?.get(h.id)
    if (spec) {
      return {
        id: h.id,
        name,
        statusLabel: 'Not installed',
        ready: false,
        detail: problem,
        detailKind: 'note',
        // A harness that needs a runtime set up first waits for the
        // human's go-ahead; the daemon refuses it otherwise.
        action: spec.needs_runtime ? 'install-runtime' : 'install',
        caps: capabilityWords(h),
      }
    }
    return {
      id: h.id,
      name,
      statusLabel: 'Not installed',
      ready: false,
      detail: problem ?? (specById ? 'No installer for this platform.' : null),
      detailKind: 'note',
      action: 'none',
      caps: capabilityWords(h),
    }
  })
}
