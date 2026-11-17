// The phone's browser (br-android 5df13bd, 56371d7, cc0080f, 546c420): a
// Chrome Android layout over native WebViews the Android shell owns. The
// view speaks main's browser store, bookmarks, history, prefs and permission
// rules rather than the branch's own copies; these tests pin that, the rules
// it draws from, and the privacy and security promises the Android side makes.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import {
  addressLine,
  agentLine,
  ago,
  barTransform,
  contextActions,
  devicesFrom,
  downloadLine,
  gridEntries,
  gridSections,
  groupLabel,
  liveGroups,
  menuRows,
  newGroup,
  openerTab,
  pageBounds,
  readWidth,
  shownPage,
  snapBox,
  swipeTab,
  thumbWanted,
} from './browser-phone.js'
import { permissionAsk, rememberedAnswer } from './browser-chrome.js'
import { getBrowserPrefs, loadBrowserPrefs } from './browser-prefs.js'
import { answerPrompt, getState, onClosed, onPermission, onTab, setState } from './browser-store.js'

// A Windows checkout can carry CRLF; the source pins below match \n.
const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8').replace(/\r\n/g, '\n')
const android = '../src-tauri/gen/android/app/src/main/java/space/moreweb/openremote/browser/'

// ---- the pure rules ----

test('the page box lands on whole device pixels, and a parked page waits far out of view', () => {
  assert.deepEqual(snapBox({ x: 0.2, y: 10.4, width: 100.3, height: 50 }, 2), { x: 0, y: 10.5, width: 100.5, height: 50 })
  const box = pageBounds({ left: 0, top: 56, width: 360, height: 600 }, { dpr: 3, bar: 56, still: false, finding: false })
  assert.deepEqual(box, { x: 0, y: 56, width: 360, height: 600, bar: 56, scale: 3, still: false, back: true, edge: 'top' })
  // Find in page holds the bar still and keeps Android's back for itself.
  const finding = pageBounds({ left: 0, top: 56, width: 360, height: 600 }, { dpr: 1, bar: 56, finding: true })
  assert.equal(finding.bar, 0)
  assert.equal(finding.back, false)
  assert.equal(pageBounds({ left: 0, top: 0, width: 360, height: 600 }, { dpr: 1, parked: true }).x, -30000)
  assert.equal(pageBounds({ left: 0, top: 0, width: 0, height: 600 }, { dpr: 1 }), null)
  assert.equal(pageBounds(null, { dpr: 1 }), null)
})

test('a page is on screen only when nothing of the console covers it', () => {
  assert.equal(shownPage({ native: 'tab-1' }, false), 'tab-1')
  assert.equal(shownPage({ native: 'tab-1' }, true), null, 'a full-screen page of the console covers it')
  assert.equal(shownPage({ native: 'tab-1', home: true }, false), null, 'the new tab page shows instead')
  assert.equal(shownPage({ native: null }, false), null)
  assert.equal(shownPage(null, false), null)
})

test('a private tab is never pictured for the grid', () => {
  assert.equal(thumbWanted({ native: 'tab-1' }), true)
  assert.equal(thumbWanted({ native: 'tab-2', private: true }), false)
  assert.equal(thumbWanted({ native: null }), false)
})

test('the grid keeps private tabs in their own section', () => {
  const tabs = [{ key: 'a' }, { key: 'b', private: true }, { key: 'c' }]
  const { normal, kept } = gridSections(tabs)
  assert.deepEqual(normal.map((t) => t.key), ['a', 'c'])
  assert.deepEqual(kept.map((t) => t.key), ['b'])
})

test('the address shows the site, and back goes to a popup\'s opener', () => {
  assert.equal(addressLine({ native: 'tab-1', url: 'https://www.example.com/a' }), 'example.com')
  assert.equal(addressLine({ native: null, url: '' }), 'Search or type a web address')
  assert.equal(addressLine({ native: 'tab-1', url: 'about:blank' }), 'about:blank')
  const tabs = [{ key: 'a', native: 'tab-1' }, { key: 'b', native: 'tab-2', opener: 'tab-1' }]
  assert.equal(openerTab(tabs, tabs[1]).key, 'a')
  assert.equal(openerTab(tabs, tabs[0]), null)
})

test('a long press offers what fits the link or picture, and no private copy from a private tab', () => {
  assert.deepEqual(contextActions({ kind: 'link', link: 'https://a.test' }, false), ['open', 'private', 'copy', 'share', 'download'])
  assert.deepEqual(contextActions({ kind: 'link', link: 'https://a.test' }, true), ['open', 'copy', 'share', 'download'])
  assert.deepEqual(contextActions({ kind: 'image', url: 'https://a.test/x.png' }, false), ['image', 'download', 'copy-image'])
  assert.deepEqual(contextActions({ kind: 'image', url: 'https://a.test/x.png', link: 'https://a.test' }, false), [
    'open',
    'private',
    'copy',
    'share',
    'image',
    'download',
    'copy-image',
  ])
  assert.deepEqual(contextActions(null, false), [])
})

test('downloads and the agent line read in plain words', () => {
  assert.equal(downloadLine({ state: 'started', received: 50, total: 200 }), 'Downloading · 25%')
  assert.equal(downloadLine({ state: 'started' }), 'Downloading')
  assert.equal(downloadLine({ state: 'failed' }), "Couldn't download")
  assert.equal(downloadLine({ state: 'done', total: 2048 }), '2 KB')
  assert.equal(downloadLine({ state: 'done' }), 'Done')
  assert.equal(agentLine({ kind: 'agent', title: 'Fix the build' }), '"Fix the build" is using this tab')
  assert.equal(agentLine({ kind: 'paused', title: 'Fix the build' }), 'You took over from "Fix the build"')
})

// ---- the store, as the phone's shell speaks to it ----

test('the phone shell\'s extra tab fields are kept, and a shell that says less blanks none of them', () => {
  setState(() => ({ tabs: [{ key: 'n', native: 'tab-1', url: 'https://a.test/' }], active: 'n', prompts: [] }))
  onTab({ id: 'tab-1', url: 'https://a.test/', progress: 40, desktop: true, crashed: false, problem: 'bad cert', isolated: true, zoom: 1.25 })
  let tab = getState().tabs[0]
  assert.equal(tab.progress, 40)
  assert.equal(tab.desktop, true)
  assert.equal(tab.problem, 'bad cert')
  assert.equal(tab.zoom, 1.25)
  onTab({ id: 'tab-1', url: 'https://a.test/b' })
  tab = getState().tabs[0]
  assert.equal(tab.url, 'https://a.test/b')
  assert.equal(tab.desktop, true, 'a desktop-shaped event leaves the phone field alone')
})

test('a page the shell closed itself leaves its tab the way a closed tab does, private ones without a trace', () => {
  setState(() => ({
    tabs: [
      { key: 'a', native: 'tab-1', url: 'https://a.test/' },
      { key: 'b', native: 'tab-2', url: 'https://secret.test/', private: true },
      { key: 'c', native: 'tab-3', url: 'https://c.test/' },
    ],
    active: 'b',
    closed: [],
    prompts: [{ tab: 'tab-2', id: 1 }],
  }))
  onClosed('tab-2')
  assert.deepEqual(getState().tabs.map((t) => t.key), ['a', 'c'])
  assert.equal(getState().active, 'c')
  assert.deepEqual(getState().closed, [], 'a private page leaves nothing to reopen')
  assert.deepEqual(getState().prompts, [])
  onClosed('tab-1')
  assert.deepEqual(getState().closed.map((e) => e.url), ['https://a.test/'])
  // A late event about the closed page never brings it back.
  onTab({ id: 'tab-1', url: 'https://a.test/' })
  assert.ok(!getState().tabs.some((t) => t.native === 'tab-1'))
  onClosed('tab-99')
  assert.deepEqual(getState().tabs.map((t) => t.key), ['c'])
})

test('a request the phone withdrew takes its row with it', async () => {
  setState(() => ({ tabs: [{ key: 'n', native: 'tab-5' }], active: 'n', prompts: [] }))
  await loadBrowserPrefs(async () => ({ settings: {}, permissions: {} }))
  await onPermission({ tab: 'tab-5', id: 7, origin: 'https://a.test', kind: 'camera', label: 'use your camera' })
  assert.equal(getState().prompts.length, 1)
  await onPermission({ tab: 'tab-5', id: 7, origin: 'https://a.test', kind: 'camera', gone: true })
  assert.deepEqual(getState().prompts, [])
})

test('the phone asks for several things at once: each is remembered and answered by its own kind', async () => {
  const kept = { 'https://a.test|camera': 'allow', 'https://a.test|microphone': 'allow', 'https://b.test|camera': 'deny' }
  assert.equal(rememberedAnswer(kept, { origin: 'https://a.test', kind: 'camera,microphone' }), 'allow')
  assert.equal(rememberedAnswer(kept, { origin: 'https://b.test', kind: 'camera,microphone' }), 'deny', 'one no is a no')
  assert.equal(rememberedAnswer({ 'https://c.test|camera': 'allow' }, { origin: 'https://c.test', kind: 'camera,microphone' }), null)
  assert.equal(permissionAsk('camera,microphone'), 'use your camera and microphone')
  assert.equal(permissionAsk('protected-media'), 'play protected content')

  await loadBrowserPrefs(async () => ({ settings: {}, permissions: {} }))
  setState(() => ({ tabs: [{ key: 'n', native: 'tab-6' }], active: 'n', prompts: [] }))
  const sent = []
  const daemon = async (method, path, body) => {
    sent.push(body)
    return null
  }
  await answerPrompt({ tab: 'tab-6', id: 1, origin: 'https://a.test', kind: 'camera,microphone' }, true, true, daemon)
  assert.deepEqual(sent, [
    { origin: 'https://a.test', kind: 'camera', state: 'allow' },
    { origin: 'https://a.test', kind: 'microphone', state: 'allow' },
  ])
  assert.equal(getBrowserPrefs().permissions['https://a.test|camera'], 'allow')
})

// ---- the view speaks main's store, not the branch's own copies ----

test('BrowserPhone uses the store, bookmarks, history, prefs and permission rules of main', () => {
  const view = read('./BrowserPhone.jsx')
  // Bookmarks: the store's rows and writes (the daemon keeps them, a refusal puts them back).
  assert.match(view, /saveBookmark\(\{ url: current\.url, title: current\.title/)
  assert.match(view, /removeBookmark\(/)
  assert.match(view, /bookmarkFor\(bookmarks, current\?\.url\)/)
  // History: the store's record, never a private page.
  assert.match(view, /if \(current\?\.private\) return/)
  assert.match(view, /recordVisit\(\{ url, title: current\?\.title \?\? '' \}\)/)
  assert.match(view, /groupByDay\(/)
  // The session and the search engine come from main's store and prefs.
  assert.match(view, /startSession\(\)/)
  assert.match(view, /browserSetting\(prefs\.settings, 'search'\)/)
  assert.match(view, /setBrowserPref\(browserCall, 'search', key\)/)
  // Suggestions are main's local rows, not the branch's route.
  assert.match(view, /suggest\(\{ typed, marks: bookmarks, visits: history \}\)/)
  // Requests: main's prompts and answer, a refusal said in words.
  assert.match(view, /answerPrompt\(prompt, allow, remember\)\.catch\(\(err\) => say\(errorCopy\(err\)\)\)/)
  // Take over, through the store's daemon call.
  assert.match(view, /browserCall\('POST', `\/browser\/tabs\/\$\{encodeURIComponent\(current\.native\)\}\/control`, \{ action \}\)/)
  // Hand a private tab to agents under the desktop's command; the share sheet is the phone's own.
  assert.match(view, /call\('browser_share', \{ tab: current\.native, on: !current\.shared \}\)/)
  assert.match(view, /shell\('browser_share_sheet', args\)/)
  assert.doesNotMatch(view, /browser_share_with_agents/)
  // Clear browsing data is main's dialog.
  assert.match(view, /<BrowserClearData /)
  // None of the branch's own copies.
  assert.doesNotMatch(view, /api\.call\(/)
  assert.doesNotMatch(view, /\/browser\/suggest/)
  assert.doesNotMatch(view, /\/browser\/history\?/)
  assert.doesNotMatch(view, /s\.asks|, asks[,\s]/)
  // A private tab is never pictured for the grid; the menu's stand-in picture goes when the menu does.
  assert.match(view, /if \(!thumbWanted\(t\)\) continue/)
  assert.match(view, /thumbWanted\(tab\)/)
  assert.match(view, /removeAttribute\('src'\)/)
  // Errors in the console's voice.
  assert.match(view, /errorCopy\(/)
  // 546c420: each step of showing a page goes only while it is still the page wanted.
  assert.match(view, /const latest = \(\) => shownNative\.current === wanted/)
  assert.match(view, /if \(latest\(\)\) await call\('browser_show'/)
})

test('the store runs on the phone and hears what only the phone says', () => {
  const store = read('./browser-store.js')
  assert.match(store, /const wired = hasTauri \? wire\(\) : Promise\.resolve\(\)/)
  assert.match(store, /listen\('browser:closed', \(\{ payload \}\) => onClosed\(payload\?\.id\)\)/)
  assert.match(store, /listen\('browser:select', \(\{ payload \}\) => focusTab\(payload\?\.id\)\)/)
})

test('the phone app shows the view, and a chat whose agent is browsing says so', () => {
  const app = read('./App.jsx')
  assert.match(app, /isMobile \? \(\s*<BrowserPhoneView/)
  const chat = read('./ChatView.jsx')
  assert.match(chat, /isMobile && isBrowsing\(chat\) && \(/)
  assert.match(chat, /requestBrowser\(\{ chatId: chat\.id \}\)/)
  assert.match(chat, /is browsing/)
  const css = read('./chatview.css')
  assert.match(css, /\.cv-browsing-dot/)
  assert.match(css, /html\[data-reduce-motion\] \.cv-browsing-dot/)
})

// ---- the Android side's promises, pinned in its source ----

test('a tab\'s WebView reads no local files, mixes no plain HTTP into a secure page, and exposes no interface', () => {
  const browser = read(`${android}Browser.kt`)
  assert.match(browser, /allowFileAccess = false/)
  assert.match(browser, /allowContentAccess = false/)
  assert.match(browser, /allowFileAccessFromFileURLs = false/)
  assert.match(browser, /allowUniversalAccessFromFileURLs = false/)
  assert.match(browser, /mixedContentMode = WebSettings\.MIXED_CONTENT_NEVER_ALLOW/)
  assert.doesNotMatch(browser, /MIXED_CONTENT_COMPATIBILITY_MODE|MIXED_CONTENT_ALWAYS_ALLOW/)
  for (const file of ['Agent.kt', 'Browser.kt', 'BrowserPlugin.kt', 'Files.kt', 'Keys.kt', 'Pages.kt', 'Permissions.kt', 'Shots.kt']) {
    assert.doesNotMatch(read(`${android}${file}`), /addJavascriptInterface/, file)
  }
  // The agent helper starts with the pages agents may use, never a private one.
  assert.match(browser, /if \(!tab\.isPrivate && WebViewFeature\.isFeatureSupported\(WebViewFeature\.DOCUMENT_START_SCRIPT\)\)/)
})

test('a private tab leaves no picture, no history and no permission behind', () => {
  const shots = read(`${android}Shots.kt`)
  assert.match(shots, /fun thumbnailNow\(tab: Tab\) \{\n\s+\/\/[^\n]*\n\s+if \(tab\.isPrivate\) return/)
  const browser = read(`${android}Browser.kt`)
  assert.match(browser, /if \(!tab\.isPrivate\) shots\.thumbnailNow\(tab\)/)
  assert.match(browser, /FLAG_SECURE/)
  // Without profiles a private tab would share the normal tabs' storage: refused.
  const plugin = read(`${android}BrowserPlugin.kt`)
  assert.match(plugin, /if \(isPrivate && !browser\.profiles\)/)
  // WebView keeps no location answer of its own; remembering is the console's, never for a private tab.
  const permissions = read(`${android}Permissions.kt`)
  assert.match(permissions, /ask\.location\?\.invoke\(ask\.origin, "geolocation" in kinds, false\)/)
  // The daemon hears a private tab only once handed over, and keeps it from history.
  const rules = read('../src-tauri/src/phone_browser.rs')
  assert.match(rules, /tab\.private = payload\.get\("private"\)\.and_then\(Value::as_bool\) != Some\(false\)/)
})

test('the phone shell keeps the desktop\'s command names and declares its own three', () => {
  const build = read('../src-tauri/build.rs')
  for (const name of ['browser_desktop_site', 'browser_share_sheet', 'browser_download']) assert.match(build, new RegExp(`"${name}"`))
  const shell = read('../src-tauri/src/mobile_browser.rs')
  assert.match(shell, /pub async fn browser_share\(app: AppHandle, tab: String, on: bool\)/)
  assert.match(shell, /pub fn browser_features\(\) -> Value/)
  const caps = JSON.parse(read('../src-tauri/capabilities/mobile.json'))
  assert.ok(caps.permissions.includes('allow-browser-share-sheet'))
  assert.ok(!caps.permissions.some((p) => p.startsWith('browser:')), 'the Kotlin plugin is reached from Rust only')
})

// ---- review fixes (round 1), pinned in the Kotlin source the build compiles ----

test('an intent: link carries no grant to the app\'s files and never points at them', () => {
  const files = read(`${android}Files.kt`)
  for (const flag of ['FLAG_GRANT_READ_URI_PERMISSION', 'FLAG_GRANT_WRITE_URI_PERMISSION', 'FLAG_GRANT_PERSISTABLE_URI_PERMISSION', 'FLAG_GRANT_PREFIX_URI_PERMISSION']) {
    assert.match(files, new RegExp(`Intent\.${flag}`), flag)
  }
  assert.match(files, /intent\.flags = intent\.flags and URI_GRANTS\.inv\(\)/)
  assert.match(files, /intent\.clipData = null/)
  assert.match(files, /if \(target == "content" \|\| target == "file"\) return/)
  // The provider shares only the app's own Downloads folder.
  const paths = read('../src-tauri/gen/android/app/src/main/res/xml/file_paths.xml')
  assert.match(paths, /<external-files-path name="downloads" path="Download\/" \/>/)
  assert.doesNotMatch(paths, /<cache-path|<external-path|<files-path|<root-path/)
})

test('reveal opens only a download', () => {
  const files = read(`${android}Files.kt`)
  assert.match(files, /if \(!file\.path\.startsWith\(downloads\.path \+ File\.separator\)\) return "that isn't a download"/)
  assert.match(files, /if \(uri\.scheme != "content" \|\| uri\.authority !in setOf\("media", "downloads", ours\)\) return "that isn't a download"/)
  assert.match(files, /\/\*\* browser_share_sheet: the phone's share sheet/)
})

test('each private session gets a fresh profile, and old ones are swept by prefix', () => {
  const browser = read(`${android}Browser.kt`)
  assert.doesNotMatch(browser, /PRIVATE_PROFILE/)
  assert.match(browser, /return privateSession \?: "\$PRIVATE_PREFIX-\$\{UUID\.randomUUID\(\)\}"\.also \{ privateSession = it \}/)
  assert.match(browser, /privateSession = null\n\s+forgetPrivate\(0\)/)
  assert.match(browser, /if \(!name\.startsWith\(PRIVATE_PREFIX\) \|\| name == privateSession\) continue/)
  assert.match(browser, /fun forgetLeftovers\(\) \{\n\s+forgetPrivate\(0\)\n\s+\}/)
})

test('the window stays secure while any private tab is open, whatever covers it', () => {
  const browser = read(`${android}Browser.kt`)
  assert.match(browser, /if \(tabs\.values\.any \{ it\.isPrivate \}\) window\.addFlags\(WindowManager\.LayoutParams\.FLAG_SECURE\)/)
  assert.doesNotMatch(browser, /shown\?\.isPrivate == true\) window\.addFlags/)
  // Set as a tab opens, re-checked as one closes and for a new activity's window.
  assert.match(browser, /tabs\[tab\.id\] = tab\n\s+secure\(\)/)
  assert.match(browser, /forgetPrivate\(0\)\n\s+\}\n\s+secure\(\)/)
})

test('Safe Browsing is on for every tab', () => {
  assert.match(read(`${android}Browser.kt`), /safeBrowsingEnabled = true/)
})

test('a blob download reads the file without installing the agent helper', () => {
  const agent = read(`${android}Agent.kt`)
  const reader = agent.slice(agent.indexOf('fun readBlob('))
  assert.doesNotMatch(reader, /__openremote_host|later\(|ready\(|browser\.scripts/)
  assert.match(reader, /delete globalThis\[\$slot\]/)
})

test('pictures free their bitmaps when they fail, and a failed clip fails', () => {
  const agent = read(`${android}Agent.kt`)
  assert.match(agent, /\?: run \{\n\s+shot\.recycle\(\)\n\s+throw Fail\("\$ref couldn't be pictured"\)/)
  assert.match(agent, /\?: run \{\n\s+sheet\.recycle\(\)\n\s+throw Fail\("the page couldn't be pictured"\)/)
  assert.match(read(`${android}Browser.kt`), /if \(scaled !== icon\) scaled\.recycle\(\)/)
  const shots = read(`${android}Shots.kt`)
  assert.match(shots, /if \(part == null\) \{\n\s+shot\.recycle\(\)\n\s+return@bitmap done\(null\)/)
})

// ---- review fixes (round 2) ----

test('a blob reader never takes more than the file\'s own bound from the page', () => {
  const agent = read(`${android}Agent.kt`)
  const reader = agent.slice(agent.indexOf('fun readBlob('))
  assert.match(reader, /val bound = max \* 4 \/ 3 \+ 4/)
  assert.match(reader, /if \(length < 0 \|\| length > bound\) \{/)
  assert.match(reader, /if \(slice\.length > CHUNK \|\| text\.length \+ slice\.length > bound\) \{/)
  // Checked before anything is appended.
  assert.ok(reader.indexOf('slice.length > CHUNK') < reader.indexOf('text.append(slice)'))
})

test('the provider keeps wry\'s camera capture folder', () => {
  const paths = read('../src-tauri/gen/android/app/src/main/res/xml/file_paths.xml')
  assert.match(paths, /<external-files-path name="pictures" path="Pictures\/" \/>/)
})

// ---- feeling like Chrome, synced across devices (2026-11-12 design) ----

test('the bar may sit at the bottom: the page then grows down, and the bar slides the other way', () => {
  const rect = { left: 0, top: 0, width: 400, height: 700 }
  assert.equal(pageBounds(rect).edge, 'top')
  assert.equal(pageBounds(rect, { bottom: true }).edge, 'bottom')
  assert.equal(barTransform(0), '')
  assert.equal(barTransform(20), 'translateY(-20px)')
  assert.equal(barTransform(20, true), 'translateY(20px)')
})

test('a sideways swipe along the bar moves one tab of the same kind, and stops at the ends', () => {
  const tabs = [{ key: 'a' }, { key: 'p', private: true }, { key: 'b' }, { key: 'c' }]
  assert.equal(swipeTab(tabs, 'a', -80, 4), 'b')
  assert.equal(swipeTab(tabs, 'b', 80, 0), 'a')
  assert.equal(swipeTab(tabs, 'c', -80, 0), null)
  assert.equal(swipeTab(tabs, 'a', 80, 0), null)
  // Too short, or more up and down than sideways, is no swipe.
  assert.equal(swipeTab(tabs, 'a', -20, 0), null)
  assert.equal(swipeTab(tabs, 'a', -80, 70), null)
  // A private tab swipes among private tabs only.
  assert.equal(swipeTab(tabs, 'p', -80, 0), null)
})

test('groups take a free colour, show as one card where their first tab stands, and go with their last tab', () => {
  const groups = { g1: { id: 'g1', name: '', color: 'blue' } }
  assert.equal(newGroup(groups, 'g2').color, 'amber')
  assert.equal(groupLabel({ name: '  ' }, 3), '3 tabs')
  assert.equal(groupLabel({ name: 'Trip' }, 1), 'Trip')
  const tabs = [
    { key: 'a' },
    { key: 'b', group: 'g1' },
    { key: 'c' },
    { key: 'd', group: 'g1' },
    { key: 'e', group: 'gone' },
    { key: 'p', private: true, group: 'g1' },
  ]
  const entries = gridEntries(tabs, groups)
  assert.deepEqual(
    entries.map((e) => e.key),
    ['a', 'group:g1', 'c', 'e'],
  )
  assert.deepEqual(
    entries[1].tabs.map((t) => t.key),
    ['b', 'd'],
  )
  assert.equal(liveGroups(groups, tabs), groups)
  assert.deepEqual(liveGroups(groups, [{ key: 'a' }, { key: 'p', private: true, group: 'g1' }]), {})
})

test('Recent tabs reads only web pages from the daemon, and says how long ago in plain words', () => {
  const devices = devicesFrom({
    devices: [
      { device: 'd1', name: 'Laptop', at: 5, tabs: [{ url: 'https://a.test/', title: 'A' }, { url: 'file:///x' }, { url: 'javascript:alert(1)' }] },
      { device: 'd2', name: '', at: 4, tabs: [{ url: 'intent://x' }] },
      { name: 'no id', tabs: [{ url: 'https://b.test/' }] },
    ],
  })
  assert.equal(devices.length, 1)
  assert.deepEqual(devices[0].tabs, [{ url: 'https://a.test/', title: 'A' }])
  assert.deepEqual(devicesFrom(null), [])
  const now = 10 * 24 * 3600_000
  assert.equal(ago(now - 10_000, now), 'Just now')
  assert.equal(ago(now - 5 * 60_000, now), '5 min ago')
  assert.equal(ago(now - 3 * 3600_000, now), '3 hr ago')
  assert.equal(ago(now - 30 * 3600_000, now), 'Yesterday')
  assert.equal(ago(now - 4 * 24 * 3600_000, now), '4 days ago')
  assert.equal(ago(0, now), '')
})

test('the reading line follows the page, and gives way to the load bar', () => {
  assert.equal(readWidth(0.5, false), 50)
  assert.equal(readWidth(2, false), 100)
  assert.equal(readWidth(0.5, true), 0)
  assert.equal(readWidth(undefined, false), 0)
})

test("the menu is Chrome's, and a private tab is offered no group", () => {
  const ids = (rows) => rows.filter((r) => r !== '-').map(([id]) => id)
  const normal = menuRows({ live: true, web: true })
  assert.deepEqual(ids(normal), [
    'new-tab',
    'new-private',
    'group',
    'history',
    'clear',
    'downloads',
    'bookmarks',
    'recent',
    'share',
    'find',
    'desktop',
    'external',
    'settings',
    'ntp',
  ])
  const priv = menuRows({ live: true, web: true, isPrivate: true })
  assert.ok(!ids(priv).includes('group'))
  assert.ok(ids(priv).includes('agents'))
  // A tab already in a group, or the new tab page, can't start one.
  assert.deepEqual(
    menuRows({ live: true, grouped: true }).find((r) => r[0] === 'group'),
    ['group', false],
  )
  assert.deepEqual(
    menuRows({}).find((r) => r[0] === 'share'),
    ['share', false],
  )
})

test('the view draws the new pieces from the store and the daemon, and private tabs stay out of them', () => {
  const view = read('./BrowserPhone.jsx')
  assert.match(view, /browserCall\('GET', '\/browser\/devices'\)/)
  assert.match(view, /browserSetting\(prefs\.settings, 'bar_bottom'\)/)
  assert.match(view, /toggle\('sync_tabs', on\)/)
  assert.match(view, /call\('browser_haptic', \{ kind \}\)/)
  // Grouping refuses a private tab, and a group's page lists normal tabs only.
  assert.match(view, /if \(!tab \|\| tab\.private\) return/)
  assert.match(view, /t\.group === openGroup && !t\.private/)
  // A shortcut is never offered for a private tab's page.
  assert.match(view, /current=\{live && !current\?\.private \? current : null\}/)
})

test("the phone shell declares its haptic command and hands the bar's edge to the page", () => {
  const build = read('../src-tauri/build.rs')
  assert.match(build, /"browser_haptic"/)
  const caps = JSON.parse(read('../src-tauri/capabilities/mobile.json'))
  assert.ok(caps.permissions.includes('allow-browser-haptic'))
  const shell = read('../src-tauri/src/mobile_browser.rs')
  assert.match(shell, /pub async fn browser_haptic\(app: AppHandle, kind: String\)/)
  assert.match(shell, /pub edge: Option<String>/)
  assert.match(read('../src-tauri/src/mobile.rs'), /crate::mobile_browser::browser_haptic/)
})

test('Kotlin: the bottom bar clips the page instead of moving it, the shell says how far down the page is, and haptics are its own', () => {
  const browser = read(`${android}Browser.kt`)
  const pages = read(`${android}Pages.kt`)
  const plugin = read(`${android}BrowserPlugin.kt`)
  assert.match(browser, /bounds\.optString\("edge", "top"\) == "bottom"/)
  assert.match(browser, /view\?\.clipBounds = if \(value > 0 && view != null\) Rect\(/)
  assert.match(browser, /tab\.view\?\.clipBounds = null/)
  assert.match(browser, /put\("read", \(tab\.page\?\.readFraction\(\) \?: 0f\)\.toDouble\(\)\)/)
  assert.match(browser, /before\?\.bottom != frame\?\.bottom/)
  assert.match(pages, /val bottom: Boolean = false/)
  assert.match(pages, /fun readFraction\(\): Float/)
  assert.match(plugin, /fun haptic\(invoke: Invoke\)/)
  assert.match(browser, /view\.performHapticFeedback\(kind\)/)
  // Still no bridge into any page.
  for (const file of ['Browser.kt', 'Pages.kt', 'BrowserPlugin.kt']) assert.doesNotMatch(read(`${android}${file}`), /addJavascriptInterface\(/)
})

test("the daemon builds open tabs from the saved session only, under the mesh's name, never into the op log", () => {
  const devices = read('../daemon/crates/openremote-daemon/src/browser/devices.rs')
  assert.match(devices, /session_tabs\(&store\.session\(\)\)/)
  assert.match(devices, /row\.get\("private"\)\.and_then\(Value::as_bool\) != Some\(true\)/)
  assert.doesNotMatch(devices, /merge_row|store\.put\(/)
  const sync = read('../daemon/crates/openremote-daemon/src/sync.rs')
  assert.match(sync, /devices\.take\(&from, &peer\.name, &note, openremote_core::now_ms\(\)\)/)
  const tabs = read('./browser-tabs.js')
  // The session the note is built from never holds a private tab.
  assert.match(tabs, /filter\(\(t\) => !t\.private && t\.url && RESTORABLE\.test\(t\.url\)\)/)
})
