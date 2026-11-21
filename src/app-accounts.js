const EXISTING = 'existing'

export function accountScope(deviceId, appId) {
  return JSON.stringify([deviceId ?? null, appId])
}

export function rememberedAppAccount(scope) {
  try {
    return localStorage.getItem(`openremote.app-account.${scope}`)
  } catch {
    return null
  }
}

export function rememberAppAccount(scope, accountId) {
  try {
    localStorage.setItem(`openremote.app-account.${scope}`, accountId)
  } catch {
    // The choice remains available in the current composer.
  }
}

export function accountChoice(accounts, preferred, existingReady) {
  const ready = accounts.filter((account) => account.status === 'ready')
  const choices = existingReady
    ? [{ id: EXISTING, label: 'Existing sign-in', status: 'ready' }, ...ready]
    : ready
  const selected = preferred
    ? choices.find((account) => account.id === preferred) ?? null
    : choices[0] ?? null
  return {
    choices,
    selected,
    blocked: Boolean(preferred && !selected),
    accountId: selected && selected.id !== EXISTING ? selected.id : null,
  }
}

// One modal owns one run. Cancelling during the start waits for that run's
// ID, so it cannot stop a login opened by another controller.
export function relayAppAccountSignIn(api, appId, accountId, onBeat = () => {}) {
  const controller = new AbortController()
  const { signal } = controller
  let runId = null
  let finished = false
  let stopRequest = null
  const stop = async () => {
    if (finished || !runId) return
    stopRequest ??= api.stopAppAccountSignIn(appId, accountId, runId).catch((error) => {
      stopRequest = null
      throw error
    })
    await stopRequest
  }
  const promise = (async () => {
    // An effect that React immediately cleans up must not send an extra login.
    await Promise.resolve()
    if (signal.aborted) return
    const start = await api.startAppAccountSignIn(appId, accountId)
    let view = start?.result ?? start
    runId = view?.run_id
    if (!runId) throw new Error('The machine did not start an account sign-in.')
    let misses = 0
    while (true) {
      if (signal.aborted) {
        if (!view.done) await stop().catch(() => {})
        return
      }
      onBeat(view)
      if (view.done) {
        finished = true
        return view
      }
      await new Promise((resolve) => setTimeout(resolve, 800))
      if (signal.aborted) return
      try {
        const next = await api.appAccountSignInView(appId, accountId, runId, signal)
        if (next?.run_id !== runId) throw new Error('The machine did not return this sign-in.')
        view = next
        misses = 0
      } catch (error) {
        if (signal.aborted) return
        misses += 1
        if (misses >= 5) throw error
      }
    }
  })()
  return {
    promise,
    stop,
    cancel() {
      controller.abort()
      stop().catch(() => {})
    },
  }
}
