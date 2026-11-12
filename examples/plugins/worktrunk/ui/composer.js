// The Worktrees button in every composer: opens the Worktrunk view, where
// the worktrees of your repositories are listed and a chat can be started
// in a new one. It writes nothing in the composer.
openremote.composer.onClick(({ input, composer }) => {
  if (input !== 'worktrees') return
  openremote.ui.openView('worktrees').catch((e) => openremote.ui.toast(String(e.message ?? e)))
  openremote.composer.setState('worktrees', { state: 'idle' }, { composer }).catch(() => {})
})
