// The harness manager's row model. Capabilities say what the daemon
// found on this machine; the machine's install specs say what it can
// install, each through its owner's own installer - every harness, on
// every desktop platform the owners ship for. The install command stays
// the daemon's business: a missing row is its name, its status, and an
// Install button.

import { harnessName } from './harness-names.js'

// The harnesses whose own login command the daemon can relay - the rest
// sign in through their own setup.
export const SIGNIN_HARNESSES = new Set(['claude', 'codex', 'grok'])

// One row per harness the daemon knows, always: the manager is the
// inventory of all six, not just the installed ones. `installable` is
// null while the machines list hasn't answered yet - a missing row then
// stays quiet rather than claiming anything for the moment.
export function harnessRows(capabilities, installable) {
  const specById =
    installable == null ? null : new Map(installable.map((spec) => [spec.harness_id, spec]))
  return (capabilities?.harnesses ?? []).map((h) => {
    const name = harnessName(h.id)
    if (h.available) {
      // The daemon's own probe words: the version it read from the CLI
      // and the path it resolved - technical content, mono in the UI.
      const detail = [h.version, h.path].filter(Boolean).join(' · ') || null
      const signedOut = h.signed_in === false
      return {
        id: h.id,
        name,
        statusLabel: signedOut ? 'Not signed in' : 'Ready',
        ready: !signedOut,
        detail,
        detailKind: 'mono',
        action: signedOut && SIGNIN_HARNESSES.has(h.id) ? 'signin' : 'none',
      }
    }
    const spec = specById?.get(h.id)
    if (spec) {
      return {
        id: h.id,
        name,
        statusLabel: 'Not installed',
        ready: false,
        detail: null,
        detailKind: 'note',
        // A harness that needs a runtime set up first waits for the
        // human's go-ahead; the daemon refuses it otherwise.
        action: spec.needs_runtime ? 'install-runtime' : 'install',
      }
    }
    return {
      id: h.id,
      name,
      statusLabel: 'Not installed',
      ready: false,
      detail: specById ? 'No installer for this platform.' : null,
      detailKind: 'note',
      action: 'none',
    }
  })
}
