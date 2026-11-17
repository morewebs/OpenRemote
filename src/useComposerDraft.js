import { useCallback, useSyncExternalStore } from 'react'
import { composerDrafts } from './composer-drafts.js'

export function useComposerDraft(key) {
  const read = useCallback(() => composerDrafts.read(key), [key])
  const text = useSyncExternalStore(composerDrafts.subscribe, read, read)
  const setText = useCallback(value => composerDrafts.write(key, value), [key])
  // Capture before awaiting a send. Completion may happen after navigation,
  // or after the person has started their next message in this composer.
  const capture = useCallback(() => {
    const sent = composerDrafts.snapshot(key)
    return () => composerDrafts.acknowledge(key, sent)
  }, [key])
  return [text, setText, capture]
}
