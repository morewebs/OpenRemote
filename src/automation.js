// Automation vocabulary: the trigger kinds with real event sources, and
// their plain sentences. The connector-backed kinds of the prototype
// (pipeline, errors, review, release) appear when their connectors do -
// until then the form offers only what can really fire.

export const KINDS = ['schedule', 'webhook']

// The prototype's kinds whose event sources still need connectors.
export const CONNECTOR_KINDS = ['pipeline', 'errors', 'review', 'release']

export function whenSentence(trigger) {
  if (trigger?.kind === 'webhook') return 'A webhook arrives'
  return `Every day at ${trigger?.time ?? '09:00'}`
}
