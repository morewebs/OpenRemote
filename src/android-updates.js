// GitHub APK builds opt in explicitly. Store builds leave this feature out.
export function githubAndroidUpdates({ native, mobile, distribution }) {
  return native === true && mobile === true && distribution === 'github'
}

function versionParts(value, stable = false) {
  const match = typeof value === 'string' && value.match(/^v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/)
  if (!match || (stable && match[4])) throw new Error('Invalid release version')
  return { numbers: match.slice(1, 4).map(Number), prerelease: Boolean(match[4]) }
}

export async function checkAndroidUpdate(currentVersion, { fetcher = globalThis.fetch, signal } = {}) {
  const response = await fetcher('https://api.github.com/repos/morewebs/OpenRemote/releases/latest', {
    headers: { Accept: 'application/vnd.github+json' },
    credentials: 'omit',
    cache: 'no-store',
    signal,
  })
  if (!response.ok) throw new Error('Could not check GitHub releases')
  const release = await response.json()
  if (release.draft !== false || release.prerelease !== false) throw new Error('Not a published stable release')
  const latest = versionParts(release.tag_name, true)
  const current = versionParts(currentVersion)
  const difference = latest.numbers.map((n, i) => n - current.numbers[i]).find((n) => n !== 0) ?? 0
  if (difference < 0 || (difference === 0 && !current.prerelease)) return null
  // Universal works for every APK flavor. Only this repository's exact
  // release asset may be opened, never a URL supplied in release notes.
  const asset = release.assets?.find((item) => item.name === 'OpenRemote-universal.apk' && item.state === 'uploaded')
  const expected = `https://github.com/morewebs/OpenRemote/releases/download/${release.tag_name}/OpenRemote-universal.apk`
  if (asset?.browser_download_url !== expected) throw new Error('The Android update is not ready')
  return { version: release.tag_name.replace(/^v/, ''), url: expected }
}
