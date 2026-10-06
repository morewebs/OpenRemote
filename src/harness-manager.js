// The harness manager's row model. Capabilities say what the daemon
// found on this machine; the machine's installable specs say what
// OpenRemote can install itself. Grok and Antigravity carry no spec on
// purpose - they install through their own roots (~/.grok/bin,
// ~/.local/bin/agy), so the manager tells that truth instead of
// offering a button it cannot honor.

import { harnessName } from './harness-names.js'

// The harnesses that have no npm install row, in their own words.
export const MANUAL_NOTE = {
  grok: 'Installs through its own setup, not npm. Run Grok Build’s installer and it appears here.',
  agy: 'Installs through its own setup, not npm. Antigravity’s installer adds it and it appears here.',
}

// One row per harness the daemon knows, always: the manager is the
// inventory of all six, not just the installed ones. `installable` is
// null while the machines list hasn't answered yet - a missing row then
// stays quiet rather than claiming "Not found" for the moment.
export function harnessRows(capabilities, installable) {
  const specById =
    installable == null ? null : new Map(installable.map((spec) => [spec.harness_id, spec]))
  return (capabilities?.harnesses ?? []).map((h) => {
    const name = harnessName(h.id)
    if (h.available) {
      // The daemon's own probe words: the version it read from the CLI
      // and the path it resolved - technical content, mono in the UI.
      const detail = [h.version, h.path].filter(Boolean).join(' · ') || null
      return {
        id: h.id,
        name,
        statusLabel: h.signed_in === false ? 'Not signed in' : 'Ready',
        ready: h.signed_in !== false,
        detail,
        detailKind: 'mono',
        action: 'none',
      }
    }
    const spec = specById?.get(h.id)
    if (spec) {
      return {
        id: h.id,
        name,
        statusLabel: 'Missing',
        ready: false,
        detail: spec.command,
        detailKind: 'mono',
        action: 'install',
      }
    }
    if (h.id in MANUAL_NOTE) {
      return {
        id: h.id,
        name,
        statusLabel: 'Missing',
        ready: false,
        detail: MANUAL_NOTE[h.id],
        detailKind: 'note',
        action: 'manual',
      }
    }
    return {
      id: h.id,
      name,
      statusLabel: 'Missing',
      ready: false,
      detail: specById ? 'Not found on this machine.' : null,
      detailKind: 'note',
      action: specById ? 'manual' : 'none',
    }
  })
}
