import { useEffect, useState } from 'react'
import { accountScope } from './app-accounts.js'

export function useAppAccounts(apiFor, deviceId, appId, { signInRevision = 0, keepListWhileRefreshing = false } = {}) {
  const [revision, setRevision] = useState(0)
  const [result, setResult] = useState(null)
  const key = accountScope(deviceId, appId)

  useEffect(() => {
    if (!appId) return
    const controller = new AbortController()
    const { signal } = controller
    apiFor(deviceId).appAccounts(appId, signal).then((response) => {
      if (!Array.isArray(response?.accounts)) throw new Error('The machine did not return its App accounts.')
      if (!signal.aborted) setResult({ key, revision, signInRevision, accounts: response.accounts })
    }).catch((error) => {
      if (!signal.aborted) setResult({ key, revision, signInRevision, error })
    })
    return () => controller.abort()
  }, [apiFor, deviceId, appId, key, revision, signInRevision])

  const scoped = appId && result?.key === key ? result : null
  const current = scoped?.revision === revision && scoped.signInRevision === signInRevision ? scoped : null
  // Keep the manager's scroll position through a save or sign-in refresh.
  // Account pickers still wait for the fresh list before offering a choice.
  const accounts = current?.accounts ?? (!current && keepListWhileRefreshing ? scoped?.accounts : null)
  return {
    accounts: accounts ?? [],
    error: current?.error ?? null,
    loading: Boolean(appId && !current && !accounts),
    refresh: () => setRevision((previous) => previous + 1),
  }
}
