// The built-in browser: Chrome's arrangement (tab strip, toolbar, address
// bar, menus) drawn by the console, with each page a native webview the
// shell lays over the placeholder below the toolbar (src-tauri/src/browser).
// The tabs live in browser-store.js, so leaving the view hides the pages
// instead of closing them.

import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import {
  ArrowClockwise,
  ArrowCounterClockwise,
  ArrowLeft,
  ArrowRight,
  ArrowSquareOut,
  ArrowsClockwise,
  CaretDown,
  CaretUp,
  Code,
  Crosshair,
  DeviceMobile,
  DotsThreeVertical,
  ClockCounterClockwise,
  DownloadSimple,
  Globe,
  Lock,
  MagnifyingGlass,
  MagnifyingGlassMinus,
  MagnifyingGlassPlus,
  Plus,
  Printer,
  Trash,
  Star,
  Warning,
  X,
} from '@phosphor-icons/react'
import { hasTauri, isMobile } from './platform.js'
import { PICKER_CANCEL, PICKER_SOURCE, formatPicked } from './browser-pick.js'
import { insertToComposer } from './composer-insert.js'
import { omnibox, security, shownAddress, suggest } from './browser-omnibox.js'
import { closeOthers, closeTab, closeToRight, cycle, canUse, extrasMissing, newTab, openBelongs, rememberClosed, stripPlan, takeClosed, tabTitle } from './browser-tabs.js'
import { adopt, call, clearHistory, getState, onKey, patchTab, recordVisit, registerReopen, removeBookmark, saveBookmark, setState, subscribe, updateBookmark } from './browser-store.js'
import { FLOATING, covered } from './browser-occlusion.js'
import { DEVICES, fitDevice, fileName, nextZoom, zoomLabel } from './browser-chrome.js'
import ContextMenu from './ContextMenu.jsx'
import { bookmarkFor, labelFor } from './browser-bookmarks.js'
import { shownVisitUrl, visitLabel } from './browser-history.js'
import { recentFor } from './browser-newtab.js'
import BrowserBookmarksBar from './BrowserBookmarksBar.jsx'
import BrowserBookmarkEditor from './BrowserBookmarkEditor.jsx'
import BrowserHistoryModal from './BrowserHistoryModal.jsx'
import BrowserSuggestions from './BrowserSuggestions.jsx'
import './browser.css'

// The app's own global keys pressed while a page had focus: handed to the
// app's shortcut layer as if typed in the console.
const APP_KEYS = { 'Mod+K': 'KeyK', 'Mod+/': 'Slash', 'Mod+N': 'KeyN', 'Mod+B': 'KeyB', 'Mod+,': 'Comma' }

function replayAppKey(chord) {
  const code = APP_KEYS[chord]
  if (!code) return false
  window.dispatchEvent(
    new KeyboardEvent('keydown', { code, key: code.replace(/^Key/, '').toLowerCase(), ctrlKey: true, bubbles: true }),
  )
  return true
}

function chordOf(e) {
  const parts = []
  if (e.ctrlKey) parts.push('Mod')
  if (e.altKey) parts.push('Alt')
  if (e.shiftKey) parts.push('Shift')
  const named = { ArrowLeft: 'Left', ArrowRight: 'Right', '+': '=', _: '-' }
  const key = named[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key)
  return [...parts, key].join('+')
}

// A box on whole device pixels: its edges rounded, not its size, so two
// boxes that touch still touch.
function snap(box) {
  const dpr = window.devicePixelRatio || 1
  const px = (v) => Math.round(v * dpr) / dpr
  const x = px(box.x)
  const y = px(box.y)
  return { x, y, width: px(box.x + box.width) - x, height: px(box.y + box.height) - y }
}

// Where a page waits while a console menu covers it: the same size (no
// relayout when it comes back), far outside the window.
function offscreen(bounds) {
  return { ...bounds, x: -30000 }
}

// Resolves after `n` frames have been painted.
function frames(n) {
  return new Promise((resolve) => {
    const step = (left) => (left ? requestAnimationFrame(() => step(left - 1)) : resolve())
    step(n)
  })
}

// A menu anchored under a toolbar button.
function anchorOf(el) {
  const r = el.getBoundingClientRect()
  return { x: r.left, y: r.bottom + 4 }
}

// ---- the view ----

export default function BrowserView({ onClosePane = null, askToShow = null }) {
  const { tabs, active, downloads, finds, closed, features, bookmarks, bookmarksStale, bookmarksNote, history } = useSyncExternalStore(subscribe, getState)
  const current = tabs.find((t) => t.key === active) ?? tabs[0]
  // What the new tab page shows under its search box: the same reading record
  // the history list draws, newest first, one row per address, six at most.
  // browser-newtab.js owns the rule, and a private tab never reaches history.
  const recent = recentFor(history)
  const pageRef = useRef(null)
  const addressRef = useRef(null)
  const findRef = useRef(null)
  const [address, setAddress] = useState('')
  const [editing, setEditing] = useState(false)
  const [suggestion, setSuggestion] = useState(-1)
  // Rows only while the field is being typed in, never on a field showing the
  // page's own address: the list is for where the person is going.
  const typedLine = editing && address && address !== shownAddress(current?.url) ? address : ''
  const suggestionRows = typedLine
    ? [...suggest({ typed: typedLine, marks: bookmarks, visits: history }), { url: typedLine, title: 'Open in a new tab', newTab: true }]
    : []
  // The page's picture while a console menu covers it: one element that
  // stays in the page, so a picture is decoded in it before it shows.
  const pictureRef = useRef(null)
  const [menu, setMenu] = useState(null)
  // The star's small editor: the bookmark it opened on, and whether the star
  // just made it (a fresh bookmark says so in the title).
  const [editor, setEditor] = useState(null)
  const [historyOpen, setHistoryOpen] = useState(false)
  // Element picking: the page waits for a click, and the pick lands in
  // the chat. `notice` is the pane's own confirm - it sits in the chrome,
  // never over the page.
  const [picking, setPicking] = useState(false)
  const [notice, setNotice] = useState(null)
  const noticeTimer = useRef(0)
  const shownNative = useRef(null)
  const mounted = useRef(true)
  // The strip's measured width, so the tabs that do not fit move behind one
  // overflow control instead of shrinking to slivers. Measured, not assumed:
  // the browser lives in a pane, and a pane is whatever width the divider
  // left it.
  const stripRef = useRef(null)
  const [stripWidth, setStripWidth] = useState(0)
  useEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    const ro = new ResizeObserver(([entry]) => setStripWidth(entry.contentRect.width))
    ro.observe(strip)
    return () => ro.disconnect()
  }, [])
  // The page a pick is waiting on: leaving the view has to stand that page's
  // picker down, or the next pick on that page answers nothing at once.
  const pickingTab = useRef(null)
  const device = current?.device ?? null
  const find = current?.find ?? null
  const findState = (current?.native && finds[current.native]) || null
  // A row is "started" only for a download the shell let through, and no
  // build lets one through today. It stays because the shell still reports
  // the state for the build that will: the button's activity is the console
  // half of that pairing, not a guess at whether anything is downloading.
  const busy = downloads.some((d) => d.state === 'started')
  // Whether this build's shell implements a browser feature: find, element
  // picking, device view and forgetting a site are Windows-only in the
  // shell, which reports what it has. An answer that never landed is not
  // optimism: the control reads as unavailable and says so, because a button
  // that draws as working and then does nothing is the worse half of the pair.
  // `canUse` and `extrasMissing` live in browser-tabs.js so the rule is tested.
  const can = (name) => canUse(features, name)
  // Said once per visit to the pane, and only when the shell answered with
  // none of the extras: the controls each name their own refusal, and this is
  // the one place that says what the missing set is.
  const saidExtras = useRef(false)
  useEffect(() => {
    if (saidExtras.current || !extrasMissing(features)) return
    saidExtras.current = true
    notify(
      "Find in page, the element picker, device view and forgetting a site need this build's shell support; they are not available here.",
    )
  }, [features])

  // The address bar follows the tab unless the user is typing in it.
  useEffect(() => {
    if (!editing) setAddress(shownAddress(current?.url))
  }, [current?.url, current?.key, editing])

  // Where the page sits: the whole placeholder, or a device's size
  // centred in it.
  // Snapped to whole device pixels, where the shell places the page, so
  // the page, its picture and the console's own pixels line up exactly.
  const rect = () => {
    const r = pageRef.current?.getBoundingClientRect()
    if (!r) return null
    const tab = getState().tabs.find((t) => t.key === getState().active)
    let box = { x: r.left, y: r.top, width: r.width, height: r.height }
    if (tab?.device) {
      const fit = fitDevice(tab.device, { width: r.width - 32, height: r.height - 32 })
      box = { x: r.left + (r.width - fit.width) / 2, y: r.top + 16, width: fit.width, height: fit.height }
    }
    return snap(box)
  }

  // While a console menu covers the page, the page waits out of view
  // (moved, not hidden: a hidden webview can show blank for a frame when
  // it comes back) and its picture stands in.
  const parked = useRef(false)

  const syncBounds = () => {
    const tab = shownNative.current
    const bounds = rect()
    if (tab && bounds) call('browser_set_bounds', { tab, bounds: parked.current ? offscreen(bounds) : bounds })
  }

  // The latest picture of the page, kept for a moment so a menu opened
  // from the toolbar can cover the page at once. One request per page at a
  // time: a pointer entering the chrome asks on every pass, and a page that
  // does not answer would otherwise start a DevTools call (and a parked
  // thread in the shell) on every pass.
  const shotRef = useRef({ tab: null, url: null, at: 0, asking: null })
  const picture$ = async (tab, maxAge) => {
    const kept = shotRef.current
    if (kept.tab === tab && Date.now() - kept.at < maxAge) return kept.url
    if (kept.asking) return kept.asking.tab === tab ? kept.asking.picture : null
    const asking = { tab, picture: null }
    asking.picture = call('browser_capture', { tab }).then((url) => {
      shotRef.current = { tab, url, at: Date.now(), asking: null }
      return url
    })
    shotRef.current = { ...kept, asking }
    return asking.picture
  }
  const precapture = () => {
    const tab = shownNative.current
    if (tab && !parked.current && can('cdp')) picture$(tab, 400)
  }

  // Show the active tab's page over the placeholder, hide the one before.
  useLayoutEffect(() => {
    if (!hasTauri) return
    const next = current?.native ?? null
    const before = shownNative.current
    shownNative.current = next
    ;(async () => {
      if (before && before !== next) await call('browser_hide', { tab: before })
      const bounds = rect()
      if (next && bounds) {
        await call('browser_set_bounds', { tab: next, bounds })
        await call('browser_show', { tab: next, focus: false })
      }
    })()
  }, [current?.native])

  // Leaving the view hides the pages; the tabs stay. Every page is hidden,
  // not just the one showing: a page can be built for a tab that closed
  // while it opened, and a page left drawn over the console has no tab
  // strip to close it with.
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
      // A pick the user was aiming at ends with the pane: the page is told to
      // stand down, so its listeners, its timer and its highlight go with it
      // instead of leaving a picker armed that answers the next pick at once.
      if (pickingTab.current) {
        call('browser_cdp', {
          tab: pickingTab.current,
          method: 'Runtime.evaluate',
          params: { expression: PICKER_CANCEL },
          wait: 'deadline',
        })
      }
      for (const t of getState().tabs) if (t.native) call('browser_hide', { tab: t.native })
    }
  }, [])

  // The page follows the placeholder: window resizes, split drags, the
  // sidebar opening, the find and device bars.
  useEffect(() => {
    if (!hasTauri || !pageRef.current) return undefined
    let frame = 0
    const sync = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(syncBounds)
    }
    const ro = new ResizeObserver(sync)
    ro.observe(pageRef.current)
    window.addEventListener('resize', sync)
    return () => {
      cancelAnimationFrame(frame)
      ro.disconnect()
      window.removeEventListener('resize', sync)
    }
  }, [])

  // A device size changes where the page sits and how it lays out. A tab
  // turning into a phone (or back) reloads, so the site sees the phone.
  const wasMobile = useRef({})
  useEffect(() => {
    const native = current?.native
    if (!native) return
    syncBounds()
    const mobile = Boolean(device?.mobile)
    const flipped = (wasMobile.current[native] ?? false) !== mobile
    wasMobile.current[native] = mobile
    ;(async () => {
      if (device) {
        const r = pageRef.current?.getBoundingClientRect()
        const fit = r ? fitDevice(device, { width: r.width - 32, height: r.height - 32 }) : { scale: 1 }
        await call('browser_viewport', {
          tab: native,
          width: device.width,
          height: device.height,
          mobile,
          scale: fit.scale,
        })
      } else {
        await call('browser_viewport', { tab: native, width: null, height: null, mobile: false, scale: null })
      }
      if (flipped) await call('browser_reload', { tab: native })
    })()
  }, [current?.native, device?.width, device?.height, device?.mobile])

  // A console menu or dialog over the page: the page's picture is decoded
  // in its element and shown first, and only once it has been painted
  // does the page step out of view; when the menu closes the page steps
  // back, is given a moment to draw, and the picture hides. A menu opening
  // straight after another (the More menu into Downloads) keeps the
  // picture up in between.
  useEffect(() => {
    if (!hasTauri) return undefined
    let swapped = null
    let leaving = 0
    let frame = 0
    const showPicture = async (url) => {
      const el = pictureRef.current
      const page = pageRef.current?.getBoundingClientRect()
      const at = rect()
      if (!el || !page || !at) return false
      // Exactly the page's own box, on the same whole device pixels: edges
      // that blend into a faint line at a fractional position cannot occur.
      el.style.left = `${at.x - page.left}px`
      el.style.top = `${at.y - page.top}px`
      el.style.width = `${at.width}px`
      el.style.height = `${at.height}px`
      if (el.getAttribute('src') !== url) el.src = url
      await el.decode().catch(() => {})
      el.classList.add('on')
      await frames(2)
      return true
    }
    const settle = async () => {
      const page = pageRef.current?.getBoundingClientRect()
      const rects = [...document.querySelectorAll(FLOATING)].map((el) => el.getBoundingClientRect())
      const tab = shownNative.current
      const cover = Boolean(tab) && covered(page, rects)
      if (cover) {
        clearTimeout(leaving)
        leaving = 0
        if (swapped) return
        swapped = tab
        const shot = await picture$(tab, 600)
        if (swapped !== tab) return
        // No picture, no swap: the menu stays behind the page rather
        // than the page going blank.
        if (!shot || !(await showPicture(shot)) || swapped !== tab) return
        parked.current = true
        syncBounds()
      } else if (swapped && !leaving) {
        leaving = setTimeout(async () => {
          leaving = 0
          swapped = null
          if (parked.current) {
            parked.current = false
            syncBounds()
            await frames(3)
          }
          if (!swapped) pictureRef.current?.classList.remove('on')
        }, 120)
      }
    }
    const check = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(settle)
    }
    const mo = new MutationObserver(check)
    mo.observe(document.body, { childList: true, subtree: true })
    return () => {
      cancelAnimationFrame(frame)
      clearTimeout(leaving)
      mo.disconnect()
      if (swapped) {
        parked.current = false
        syncBounds()
      }
    }
  }, [])

  // ---- actions ----

  const activeTab = () => getState().tabs.find((t) => t.key === getState().active)

  const go = async (line) => {
    const url = omnibox(line)
    if (!url) return
    setEditing(false)
    addressRef.current?.blur()
    const tab = activeTab()
    if (!tab) return
    if (tab.native) {
      await call('browser_navigate', { tab: tab.native, url })
      return
    }
    const bounds = rect()
    if (!bounds) return
    patchTab(tab.key, { url, loading: true })
    // The page is built hidden: the layout effect places and shows it once
    // the tab has it, so the console is the only thing that decides a page
    // is visible.
    const native = await call('browser_open', { url, bounds, show: false })
    // No page, or a page nothing owns any more: the tab may have been closed,
    // or the whole pane, while the shell was building it, and a page that
    // lands anyway has no tab strip to close it with and floats over the
    // console. Either way the tab is no longer loading anything, or it would
    // spin for the rest of the session.
    if (!native || !openBelongs(getState().tabs, tab.key, mounted.current)) {
      patchTab(tab.key, { loading: false })
      if (native) call('browser_close', { tab: native })
      return
    }
    // The layout effect owns shownNative and shows the page as the tab gains
    // it; an open landing in a background tab must not claim the ref, or the
    // page the pane is showing stops being placed and pictured.
    adopt(tab.key, native)
  }

  const openTab = () => {
    const tab = newTab()
    setState((s) => ({ tabs: [...s.tabs, tab], active: tab.key }))
    setEditing(true)
    setAddress('')
    requestAnimationFrame(() => addressRef.current?.focus())
  }

  // ---- bookmarks ----

  // A suggestion row: the address it names, here or in a new tab.
  const pick = (row, { newTab: inNewTab = false } = {}) => {
    if (!row?.url) return
    setSuggestion(-1)
    setEditing(false)
    openUrl(omnibox(row.url), { newTab: inNewTab || row.newTab })
  }

  // A bookmark's address, here or in a new tab (the bar's middle click and
  // Ctrl+click come through the second form).
  const openUrl = (url, { newTab: inNewTab = false } = {}) => {
    if (!url) return
    if (!inNewTab) {
      go(url)
      return
    }
    const tab = newTab()
    setState((s) => ({ tabs: [...s.tabs, tab], active: tab.key }))
    requestAnimationFrame(() => go(url))
  }

  // The star (and Ctrl+D): a page that is not bookmarked is bookmarked on the
  // bar and the editor opens; a page that is bookmarked opens the editor for
  // the bookmark that holds it. A tab with no address has nothing to keep.
  const star = () => {
    const tab = activeTab()
    if (editor || !tab?.url) return
    const kept = bookmarkFor(bookmarks, tab.url)
    if (kept) {
      setEditor({ mark: kept, fresh: false })
      return
    }
    const saved = saveBookmark({ url: tab.url, title: tabTitle(tab), folder: 'bar' })
    if (saved) setEditor({ mark: saved, fresh: true })
  }

  const finishEditing = ({ title, folder }) => {
    if (!editor) return
    updateBookmark(editor.mark.id, { title, folder })
    setEditor(null)
  }

  const removeEdited = () => {
    if (!editor) return
    removeBookmark(editor.mark.id)
    setEditor(null)
  }

  // A visit is recorded when the pane shows a page, not on every picture
  // update: the address and its title, and the pure rule folds the title that
  // lands a moment later into the same visit. A tab with no address (the new
  // tab page) has not been anywhere yet.
  useEffect(() => {
    // A private tab is kept from this window's history: it is
    // recorded nowhere, so the record cannot name a page the person read in
    // private, and the new tab page's recent list cannot either. The shell
    // says which tabs those are, and browser-store carries the flag.
    if (current?.private) return
    const url = current?.url
    if (!url || !/^https?:\/\//i.test(url)) return
    recordVisit({ url, title: current?.title ?? '' })
  }, [active, current?.url, current?.title, current?.private])

  const close = (key) => {
    const tab = getState().tabs.find((t) => t.key === key)
    if (tab?.native) call('browser_close', { tab: tab.native })
    setState((s) => {
      const next = closeTab(s.tabs, key, s.active)
      return { tabs: next.tabs, active: next.active, closed: rememberClosed(s.closed, next.closed) }
    })
  }

  // Close several at once, keeping `keep` if it is still in the list: the
  // strip's own two bulk commands, and the page of every closed tab really
  // closes in the shell, not only in this list.
  const closeMany = (keys, keep = null) => {
    if (!keys.length) return
    const gone = new Set(keys)
    for (const t of getState().tabs) if (gone.has(t.key) && t.native) call('browser_close', { tab: t.native })
    setState((s) => {
      let closed = s.closed
      for (const t of s.tabs) if (gone.has(t.key)) closed = rememberClosed(closed, t)
      const rest = s.tabs.filter((t) => !gone.has(t.key))
      if (!rest.length) {
        const fresh = newTab()
        return { tabs: [fresh], active: fresh.key, closed }
      }
      const active = rest.some((t) => t.key === s.active)
        ? s.active
        : rest.some((t) => t.key === keep)
          ? keep
          : rest[0].key
      return { tabs: rest, active, closed }
    })
  }

  // A closed tab's shell page is gone with the close, so reopening builds a
  // new tab and takes it to the address the tab had. The new tab page leaves
  // nothing behind, so an empty list is the only nothing-to-do case.
  const reopen = () => {
    const taken = takeClosed(getState().closed)
    if (!taken) return
    const tab = newTab()
    setState((s) => ({ closed: taken.rest, tabs: [...s.tabs, tab], active: tab.key }))
    requestAnimationFrame(() => go(taken.entry.url))
  }

  // The palette's "Reopen closed tab" asks the store, and the store asks the
  // mounted view: only the view can build the page and place it. The ref keeps
  // the registration pointing at the latest render's `reopen`.
  const reopenRef = useRef(reopen)
  reopenRef.current = reopen
  useEffect(() => registerReopen(() => reopenRef.current()), [])

  // A chat asked for the browser, or its agent opened a tab: show that tab, or open the
  // address it named. The request arrives as a value (the shell owns it), so a repeat of
  // the same request is a repeat of the same intent and is answered once per value.
  const ask = useRef(null)
  const goRef = useRef(null)
  goRef.current = go
  useEffect(() => {
    if (!askToShow || askToShow === ask.current) return
    ask.current = askToShow
    if (askToShow.tab) {
      const shown = getState().tabs.find((t) => t.native === askToShow.tab || t.key === askToShow.tab)
      if (shown) setState((s) => ({ active: shown.key }))
      return
    }
    if (askToShow.url) {
      const fresh = newTab()
      setState((s) => ({ tabs: [...s.tabs, fresh], active: fresh.key }))
      requestAnimationFrame(() => goRef.current?.(askToShow.url))
    }
  }, [askToShow])

  const onNative = (command, extra = {}) => {
    const native = activeTab()?.native
    if (native) return call(command, { tab: native, ...extra })
    return Promise.resolve(null)
  }

  const zoom = (dir) => {
    const tab = activeTab()
    if (!tab?.native) return
    const factor = nextZoom(tab.zoom ?? 1, dir)
    // The record follows the shell, not the wish: a refused zoom used to leave the
    // address bar's badge, the menu's "Actual size" entry and Ctrl+0 all describing
    // a zoom the page never took, so the console argued with the page about how big
    // it was. `call` answers null when the command did not land.
    call('browser_zoom', { tab: tab.native, factor }).then((done) => {
      if (done != null) patchTab(tab.key, { zoom: factor })
    })
  }

  const focusAddress = () => {
    setEditing(true)
    requestAnimationFrame(() => {
      addressRef.current?.focus()
      addressRef.current?.select()
    })
  }

  const openFind = () => {
    const tab = activeTab()
    if (!tab?.native) return
    if (!can('find')) {
      notify('Find in page is not in this build yet')
      return
    }
    patchTab(tab.key, { find: { term: tab.find?.term ?? '' } })
    requestAnimationFrame(() => {
      findRef.current?.focus()
      findRef.current?.select()
    })
  }

  const runFind = (term, step) => {
    const tab = activeTab()
    if (!tab?.native) return
    patchTab(tab.key, { find: { term } })
    call('browser_find', { tab: tab.native, term, step })
  }

  const closeFind = () => {
    const tab = activeTab()
    if (!tab) return
    patchTab(tab.key, { find: null })
    if (tab.native) call('browser_find', { tab: tab.native, term: '', step: 0 })
  }

  const notify = (text) => {
    setNotice(text)
    clearTimeout(noticeTimer.current)
    noticeTimer.current = setTimeout(() => setNotice(null), 4000)
  }

  // The shell refuses every download a page starts: a page must never write
  // a file the user did not ask for. The downloads menu is not where anyone
  // looks, so the pane says it too. The ref starts at whatever was already
  // listed, so reopening the pane does not replay old ones.
  const blocked = downloads.find((d) => d.state === 'blocked') ?? null
  const saidBlocked = useRef(blocked?.id ?? 0)
  useEffect(() => {
    if (!blocked || blocked.id === saidBlocked.current) return
    saidBlocked.current = blocked.id
    notify(`Blocked a download: ${fileName(blocked.path ?? blocked.url)}. Open the link in your default browser to save it.`)
  }, [blocked?.id])

  // Stand a page's picker down: the page clears its flag, its highlight, its
  // timer and its listeners. A page with no picker running ignores the flag
  // until the next pick clears it again.
  const standDownPick = (native) => {
    if (!native) return
    call('browser_cdp', {
      tab: native,
      method: 'Runtime.evaluate',
      params: { expression: PICKER_CANCEL },
      wait: 'deadline',
    })
  }

  // Point at an element on the page and attach it to the chat: its
  // address, its kind and its words land in the composer's draft, where
  // the agent reads them. The picker runs in the page; Esc backs out.
  const startPick = async () => {
    const tab = activeTab()
    if (!tab?.native || picking) return
    if (!can('cdp')) {
      notify('Selecting an element is not in this build yet')
      return
    }
    setPicking(true)
    pickingTab.current = tab.native
    try {
      const res = await call('browser_cdp', {
        tab: tab.native,
        method: 'Runtime.evaluate',
        params: { expression: PICKER_SOURCE, awaitPromise: true, returnByValue: true },
        // This call is not the console asking the page a question, it is the
        // console waiting for the person to choose, so the shell applies no
        // deadline to it: no timer may end a pick. The calls that do carry a
        // deadline are the ones where the page owes an answer (screenshots,
        // viewport changes), which must not hold the pane.
        wait: 'user',
      })
      // No answer at all: the tab closed under the pick. Say so, so the strip
      // is not left armed with nothing coming.
      if (!res) {
        notify('The page went away - the pick was cancelled')
        return
      }
      const pick = res.result?.subtype === 'error' ? null : (res.result?.value ?? null)
      // No element: the user pressed Esc, or the page still had a picker
      // armed from an earlier pick. Standing down either way leaves the page
      // ready for the next one instead of answering it with nothing.
      if (!pick) {
        standDownPick(tab.native)
        return
      }
      const line = formatPicked({ ...pick, url: tab.url, title: tab.title })
      if (insertToComposer(line)) notify('Attached to the chat')
      else notify('No chat open - open one first, then pick again')
    } catch {
      // The console could not reach the shell at all: back out, and say it.
      notify('The page could not be reached - the pick was cancelled')
    } finally {
      pickingTab.current = null
      setPicking(false)
    }
  }

  const cancelPick = () => {
    // The page that is picking, which is not necessarily the active tab any
    // more: the pick is told to stand down where the picker is, not where the
    // user happens to be looking.
    const native = pickingTab.current ?? activeTab()?.native
    pickingTab.current = null
    setPicking(false)
    // The page's own Esc already stood down if it heard it; this is for the
    // bar's Cancel, and for a pick the console gave up on, which the page
    // cannot hear.
    standDownPick(native)
  }

  const toggleDevice = () => {
    const tab = activeTab()
    if (!tab?.native) return
    if (!can('viewport')) {
      notify('Device view is not in this build yet')
      return
    }
    patchTab(tab.key, { device: tab.device ? null : { ...DEVICES[0] } })
  }

  const setDevice = (change) => {
    const tab = activeTab()
    if (tab?.device) patchTab(tab.key, { device: { ...tab.device, ...change } })
  }

  // The browser's keys: from a page (the shell takes them before the page
  // does) or typed in the browser's own chrome.
  const keys = useRef({})
  keys.current = {
    'Mod+T': openTab,
    'Mod+W': () => close(getState().active),
    'Mod+L': focusAddress,
    'Mod+D': star,
    'Mod+R': () => onNative('browser_reload'),
    F5: () => onNative('browser_reload'),
    'Mod+F': openFind,
    'Mod+J': (e) => setMenu({ kind: 'downloads', ...anchorFor('downloads', e) }),
    F12: () => (can('devtools') ? onNative('browser_devtools') : notify('Developer tools are not in this build')),
    'Mod+=': () => zoom(1),
    'Mod+-': () => zoom(-1),
    'Mod+0': () => zoom(0),
    'Mod+Tab': () => setState((s) => ({ active: cycle(s.tabs, s.active, 1) })),
    'Mod+Shift+Tab': () => setState((s) => ({ active: cycle(s.tabs, s.active, -1) })),
    'Mod+Shift+T': reopen,
    'Alt+Left': () => onNative('browser_back'),
    'Alt+Right': () => onNative('browser_forward'),
  }
  const anchorFor = (name) => {
    const el = document.querySelector(`[data-bw-anchor="${name}"]`)
    return el ? anchorOf(el) : { x: 0, y: 0 }
  }
  useEffect(() => {
    const fromPage = (chord) => {
      if (replayAppKey(chord)) return
      keys.current[chord]?.()
    }
    const unhook = onKey(fromPage)
    const fromChrome = (e) => {
      const fn = keys.current[chordOf(e)]
      if (!fn) return
      e.preventDefault()
      e.stopPropagation()
      fn()
    }
    window.addEventListener('keydown', fromChrome, true)
    return () => {
      unhook()
      window.removeEventListener('keydown', fromChrome, true)
    }
  }, [])

  if (!hasTauri || isMobile) {
    return (
      <div className="bw-unavailable">
        <Globe size={28} />
        <p>{isMobile ? 'The browser comes to the phone in its own build.' : 'The browser runs in the desktop app.'}</p>
      </div>
    )
  }

  const lock = security(current?.url)
  const live = Boolean(current?.native)
  const zoomed = (current?.zoom ?? 1) !== 1

  const menuItems = () => {
    if (!menu) return []
    if (menu.kind === 'main') {
      return [
        { label: 'New tab', icon: <Plus size={14} />, hint: 'Ctrl+T', onSelect: openTab },
        { label: 'Reopen closed tab', icon: <ArrowCounterClockwise size={14} />, hint: 'Ctrl+Shift+T', disabled: !closed.length, onSelect: reopen },
        { divider: true },
        { label: 'Zoom in', icon: <MagnifyingGlassPlus size={14} />, hint: 'Ctrl+=', disabled: !live, onSelect: () => zoom(1) },
        { label: 'Zoom out', icon: <MagnifyingGlassMinus size={14} />, hint: 'Ctrl+-', disabled: !live, onSelect: () => zoom(-1) },
        { label: `Actual size (${zoomLabel(current?.zoom ?? 1)})`, icon: <ArrowsClockwise size={14} />, hint: 'Ctrl+0', disabled: !live || !zoomed, onSelect: () => zoom(0) },
        { divider: true },
        {
          label: 'Find in page',
          icon: <MagnifyingGlass size={14} />,
          hint: can('find') ? 'Ctrl+F' : 'Not in this build',
          disabled: !live || !can('find'),
          onSelect: openFind,
        },
        {
          label: device ? 'Leave device view' : 'Device view',
          icon: <DeviceMobile size={14} />,
          hint: can('viewport') ? '' : 'Not in this build',
          disabled: !live || !can('viewport'),
          onSelect: toggleDevice,
        },
        {
          label: 'Print',
          icon: <Printer size={14} />,
          hint: can('print') ? '' : 'Not in this build',
          disabled: !live || !can('print'),
          onSelect: () => onNative('browser_print'),
        },
        { divider: true },
        { label: 'Downloads', icon: <DownloadSimple size={14} />, hint: 'Ctrl+J', onSelect: () => setTimeout(() => setMenu({ kind: 'downloads', ...anchorFor('downloads') }), 0) },
        { label: 'History', icon: <ClockCounterClockwise size={14} />, disabled: !history.length, onSelect: () => setHistoryOpen(true) },
        { label: 'Open in your default browser', icon: <ArrowSquareOut size={14} />, disabled: !live || !/^https?:/.test(current?.url ?? ''), onSelect: () => call('browser_open_external', { url: current.url }) },
        ...(import.meta.env.DEV
          ? [{
              label: 'Developer tools',
              icon: <Code size={14} />,
              hint: can('devtools') ? 'F12' : 'Not in this build',
              disabled: !live || !can('devtools'),
              onSelect: () => onNative('browser_devtools'),
            }]
          : []),
      ]
    }
    if (menu.kind === 'site') {
      let origin = ''
      try {
        origin = new URL(current.url).host
      } catch {
        origin = current?.url ?? ''
      }
      const said = {
        secure: 'Connection is secure',
        local: 'A page on this computer',
        insecure: 'Not secure: this site sends what you type unencrypted',
        none: 'A local page',
      }[lock]
      return [
        { header: origin },
        { label: said, icon: lock === 'insecure' ? <Warning size={14} /> : <Lock size={14} />, disabled: true },
        { divider: true },
        ...(zoomed ? [{ label: `Zoom ${zoomLabel(current.zoom)}: reset`, icon: <ArrowsClockwise size={14} />, onSelect: () => zoom(0) }] : []),
        { label: 'Clear cookies and site data', icon: <Trash size={14} />, hint: can('clearSite') ? '' : 'Not in this build', danger: true, disabled: !can('clearSite'), onSelect: () => onNative('browser_clear_site') },
      ]
    }
    if (menu.kind === 'mark') {
      const m = menu.mark
      return [
        { header: labelFor(m) },
        { label: 'Open', icon: <ArrowRight size={14} />, onSelect: () => openUrl(m.url) },
        { label: 'Open in a new tab', icon: <Plus size={14} />, onSelect: () => openUrl(m.url, { newTab: true }) },
        { divider: true },
        { label: 'Edit', onSelect: () => setEditor({ mark: m, fresh: false }) },
        { label: 'Remove', icon: <Trash size={14} />, danger: true, onSelect: () => removeBookmark(m.id) },
      ]
    }
    if (menu.kind === 'marks') {
      return [
        { header: 'Bookmarks' },
        ...(menu.marks ?? []).map((m) => ({ label: labelFor(m), onSelect: () => openUrl(m.url) })),
      ]
    }
    if (menu.kind === 'downloads') {
      if (!downloads.length) return [{ header: 'Downloads' }, { label: 'Nothing downloaded yet', disabled: true }]
      return [
        { header: 'Downloads' },
        ...downloads.slice(0, 12).map((d) => {
          // A blocked attempt has nothing on disk to show, and the notice
          // tells the user to open the link in their own browser: this row
          // is the way to do it from here.
          const refused = d.state === 'blocked'
          return {
            label: fileName(d.path ?? d.url),
            icon: d.state === 'failed' ? <Warning size={14} /> : <DownloadSimple size={14} />,
            hint: d.state === 'started' ? 'Downloading' : refused ? 'Blocked: open in your default browser' : d.state === 'failed' ? 'Failed' : 'Show in folder',
            disabled: refused ? false : d.state !== 'done' || !d.path,
            onSelect: () => (refused ? call('browser_open_external', { url: d.url }) : call('browser_reveal', { path: d.path })),
          }
        }),
        // The store keeps more than the menu draws, and a list that just stops is
        // a list that says it is everything: the row below names what is not here
        // so twelve of fifty does not read as fifty.
        ...(downloads.length > 12
          ? [{ label: `and ${downloads.length - 12} more this session`, hint: 'The menu shows the newest twelve', disabled: true }]
          : []),
      ]
    }
    if (menu.kind === 'tab') {
      const key = menu.key
      const right = closeToRight(tabs, key)
      return [
        { header: tabTitle(tabs.find((t) => t.key === key) ?? {}) },
        { label: 'Close tab', hint: 'Ctrl+W', onSelect: () => close(key) },
        {
          label: 'Close others',
          disabled: tabs.length < 2,
          onSelect: () => closeMany(closeOthers(tabs, key), key),
        },
        {
          label: 'Close to the right',
          disabled: right.length === 0,
          onSelect: () => closeMany(right, key),
        },
        {
          label: 'Reopen closed tab',
          hint: 'Ctrl+Shift+T',
          disabled: !closed.length,
          onSelect: reopen,
        },
      ]
    }
    if (menu.kind === 'tabs') {
      return [
        { header: `${tabs.length} open ${tabs.length === 1 ? 'tab' : 'tabs'}` },
        ...tabs.map((t) => ({
          label: tabTitle(t),
          icon: t.favicon ? <img className="bw-menu-icon" src={t.favicon} alt="" /> : <Globe size={14} />,
          hint: t.key === active ? 'Showing' : '',
          onSelect: () => setState(() => ({ active: t.key })),
        })),
      ]
    }
    return []
  }

  // What the strip draws: whole tabs only, with what does not fit behind the
  // one overflow control, and the active tab always inside the window. The
  // counting is a pure rule in browser-tabs.js so it can be tested without a
  // window.
  const plan = stripPlan({
    width: stripWidth,
    count: tabs.length,
    active: Math.max(0, tabs.findIndex((t) => t.key === active)),
  })
  const shownTabs = tabs.slice(plan.start, plan.start + plan.shown)

  return (
    <div className="bw">
      <div className="bw-tabs" role="tablist" aria-label="Tabs" onPointerEnter={precapture}>
        <div className="bw-tabs-scroll" ref={stripRef}>
          {shownTabs.map((t) => (
            <div
              key={t.key}
              role="tab"
              aria-selected={t.key === active}
              className={`bw-tab${t.key === active ? ' on' : ''}`}
              onMouseDown={(e) => {
                if (e.button === 1) {
                  e.preventDefault()
                  close(t.key)
                } else if (e.button === 0) setState(() => ({ active: t.key }))
              }}
              onContextMenu={(e) => {
                e.preventDefault()
                setMenu({ kind: 'tab', key: t.key, ...anchorOf(e.currentTarget) })
              }}
              title={tabTitle(t)}
            >
              {t.loading ? (
                <span className="bw-spinner" aria-label="Loading" />
              ) : t.favicon ? (
                <img className="bw-favicon" src={t.favicon} alt="" />
              ) : (
                <Globe size={14} className="bw-tab-icon" />
              )}
              <span className="bw-tab-title">{tabTitle(t)}</span>
              <button
                type="button"
                className="bw-tab-close"
                title="Close tab (Ctrl+W)"
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => close(t.key)}
              >
                <X size={11} weight="bold" />
              </button>
            </div>
          ))}
          <button type="button" className="bw-newtab" title="New tab (Ctrl+T)" onClick={openTab}>
            <Plus size={14} />
          </button>
        </div>
        <button
          type="button"
          className={plan.hidden ? 'bw-btn bw-tabsearch is-more' : 'bw-btn bw-tabsearch'}
          title={plan.hidden ? `${plan.hidden} more ${plan.hidden === 1 ? 'tab' : 'tabs'}` : 'Your tabs'}
          aria-label={plan.hidden ? `${plan.hidden} more tabs` : 'Your tabs'}
          onClick={(e) => setMenu({ kind: 'tabs', ...anchorOf(e.currentTarget) })}
        >
          <CaretDown size={14} />
          {plan.hidden > 0 && <span className="bw-tab-more-count">{plan.hidden}</span>}
        </button>
        {onClosePane && (
          <button type="button" className="bw-btn bw-tabsearch" title="Close the browser pane" onClick={onClosePane}>
            <X size={14} />
          </button>
        )}
      </div>

      {bookmarksStale && (
        // The rows above are this window's last cache, not what the daemon
        // holds: said plainly rather than showing a save that did not happen,
        // and with the reason the store found (no answer, no route, or the
        // daemon's own refusal).
        <p className="bw-marks-empty" role="status">
          {bookmarksNote ?? 'Bookmarks are the list this window last saw: the daemon did not answer.'}
        </p>
      )}
      {bookmarks.length > 0 && (
        <BrowserBookmarksBar
          marks={bookmarks}
          precapture={precapture}
          onOpen={openUrl}
          onMenu={(m, e) => setMenu({ kind: 'mark', mark: m, x: e.clientX, y: e.clientY })}
          onFolderMenu={(list, e) => setMenu({ kind: 'marks', marks: list, x: e.clientX, y: e.clientY })}
        />
      )}

      <div className="bw-bar" onPointerEnter={precapture}>
        <button type="button" className="bw-btn" title="Back (Alt+Left)" disabled={!current?.canBack} onClick={() => onNative('browser_back')}>
          <ArrowLeft size={16} />
        </button>
        <button type="button" className="bw-btn" title="Forward (Alt+Right)" disabled={!current?.canForward} onClick={() => onNative('browser_forward')}>
          <ArrowRight size={16} />
        </button>
        {current?.loading && live ? (
          <button type="button" className="bw-btn" title="Stop loading" onClick={() => onNative('browser_stop')}>
            <X size={16} />
          </button>
        ) : (
          <button type="button" className="bw-btn" title="Reload (Ctrl+R)" disabled={!live} onClick={() => onNative('browser_reload')}>
            <ArrowClockwise size={16} />
          </button>
        )}
        <form
          className="bw-address"
          onSubmit={(e) => {
            e.preventDefault()
            go(address)
          }}
        >
          <button
            type="button"
            className={`bw-site ${lock}`}
            title="Site information"
            disabled={!live}
            onClick={(e) => setMenu({ kind: 'site', ...anchorOf(e.currentTarget) })}
          >
            {lock === 'insecure' ? <Warning size={14} /> : lock === 'none' ? <Globe size={14} /> : <Lock size={14} />}
            {lock === 'insecure' && <span className="bw-site-word">Not secure</span>}
          </button>
          <input
            ref={addressRef}
            value={address}
            placeholder="Search Google or type a URL"
            spellCheck={false}
            role="combobox"
            aria-label="Address"
            aria-autocomplete="list"
            aria-expanded={suggestionRows.length > 0}
            aria-controls="bw-suggest"
            aria-activedescendant={suggestion >= 0 ? `bw-suggest-${suggestion}` : undefined}
            onChange={(e) => {
              setEditing(true)
              setSuggestion(-1)
              setAddress(e.target.value)
            }}
            onFocus={(e) => {
              setEditing(true)
              e.target.select()
            }}
            onBlur={() => setEditing(false)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setSuggestion(-1)
                setEditing(false)
                setAddress(shownAddress(current?.url))
                e.currentTarget.blur()
                return
              }
              // The list is driven from the field, which keeps the focus: the
              // arrows move the highlight and Enter opens it, so a person who
              // never touches the mouse can still use the suggestions.
              if (!suggestionRows.length) return
              if (e.key === 'ArrowDown') {
                e.preventDefault()
                setSuggestion((i) => (i + 1) % suggestionRows.length)
              } else if (e.key === 'ArrowUp') {
                e.preventDefault()
                setSuggestion((i) => (i <= 0 ? suggestionRows.length - 1 : i - 1))
              } else if (e.key === 'Enter' && suggestion >= 0) {
                e.preventDefault()
                pick(suggestionRows[suggestion], { newTab: e.ctrlKey || e.metaKey })
              }
            }}
          />
          {zoomed && (
            <button type="button" className="bw-zoom" title="Reset zoom (Ctrl+0)" onClick={() => zoom(0)}>
              {zoomLabel(current.zoom)}
            </button>
          )}
          <button
            type="button"
            data-bw-anchor="star"
            className={current?.url && bookmarkFor(bookmarks, current.url) ? 'bw-btn bw-star on' : 'bw-btn bw-star'}
            title="Bookmark this page (Ctrl+D)"
            aria-label={current?.url && bookmarkFor(bookmarks, current.url) ? 'Edit this page bookmark' : 'Bookmark this page'}
            aria-pressed={Boolean(current?.url && bookmarkFor(bookmarks, current.url))}
            disabled={!current?.url}
            onClick={star}
          >
            <Star size={15} weight={current?.url && bookmarkFor(bookmarks, current.url) ? 'fill' : 'regular'} />
          </button>
        </form>
        <BrowserSuggestions id="bw-suggest" rows={suggestionRows} active={suggestion} onPick={pick} />
        <button
          type="button"
          className={`bw-btn${device ? ' on' : ''}`}
          title={can('viewport') ? 'Device view' : 'Device view is not in this build yet'}
          disabled={!live || !can('viewport')}
          onClick={toggleDevice}
        >
          <DeviceMobile size={17} />
        </button>
        <button
          type="button"
          className={`bw-btn${picking ? ' on' : ''}`}
          title={can('cdp') ? 'Select an element to attach to the chat' : 'Selecting an element is not in this build yet'}
          disabled={!live || !can('cdp')}
          onClick={picking ? cancelPick : startPick}
        >
          <Crosshair size={17} />
        </button>
        <button
          type="button"
          className={`bw-btn${busy ? ' busy' : ''}`}
          data-bw-anchor="downloads"
          title="Downloads (Ctrl+J)"
          onClick={(e) => setMenu({ kind: 'downloads', ...anchorOf(e.currentTarget) })}
        >
          <DownloadSimple size={17} />
        </button>
        <button
          type="button"
          className="bw-btn"
          title="More"
          onClick={(e) => setMenu({ kind: 'main', ...anchorOf(e.currentTarget) })}
        >
          <DotsThreeVertical size={18} weight="bold" />
        </button>
      </div>

      {picking && live && (
        <div className="bw-strip">
          <Crosshair size={14} className="bw-strip-icon" />
          <span>Click an element on the page - Esc backs out</span>
          <span className="bw-strip-fill" />
          <button type="button" className="bw-btn" title="Stop picking" onClick={cancelPick}>
            <X size={14} />
          </button>
        </div>
      )}

      {notice && (
        <div className="bw-strip">
          <span>{notice}</span>
          <span className="bw-strip-fill" />
          <button type="button" className="bw-btn" title="Dismiss" onClick={() => setNotice(null)}>
            <X size={14} />
          </button>
        </div>
      )}

      {find && live && (
        <div className="bw-strip">
          <MagnifyingGlass size={14} className="bw-strip-icon" />
          <input
            ref={findRef}
            className="bw-find-input"
            value={find.term}
            placeholder="Find in page"
            spellCheck={false}
            onChange={(e) => runFind(e.target.value, 0)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                runFind(find.term, e.shiftKey ? -1 : 1)
              } else if (e.key === 'Escape') {
                e.preventDefault()
                closeFind()
              }
            }}
          />
          {/* The count is the shell's answer, so silence until there is one:
              printing "No matches" while the term is still travelling to the
              engine tells a person their search found nothing before the
              engine has looked. */}
          <span className="bw-find-count">
            {find.term
              ? findState
                ? findState.count
                  ? `${findState.active} of ${findState.count}`
                  : 'No matches'
                : ''
              : ''}
          </span>
          <button type="button" className="bw-btn" title="Previous (Shift+Enter)" onClick={() => runFind(find.term, -1)}>
            <CaretUp size={14} />
          </button>
          <button type="button" className="bw-btn" title="Next (Enter)" onClick={() => runFind(find.term, 1)}>
            <CaretDown size={14} />
          </button>
          <button type="button" className="bw-btn" title="Close (Esc)" onClick={closeFind}>
            <X size={14} />
          </button>
        </div>
      )}

      {device && live && (
        <div className="bw-strip">
          <DeviceMobile size={14} className="bw-strip-icon" />
          {DEVICES.map((d) => (
            <button
              key={d.name}
              type="button"
              className={`bw-chip${device.name === d.name ? ' on' : ''}`}
              onClick={() => setDevice({ ...d })}
            >
              {d.name}
            </button>
          ))}
          <span className="bw-size">
            <input
              type="number"
              value={device.width}
              min={200}
              max={3840}
              onChange={(e) => setDevice({ name: 'Custom', width: Number(e.target.value) || device.width })}
            />
            <span>x</span>
            <input
              type="number"
              value={device.height}
              min={200}
              max={2160}
              onChange={(e) => setDevice({ name: 'Custom', height: Number(e.target.value) || device.height })}
            />
          </span>
          <button
            type="button"
            className="bw-btn"
            title="Rotate"
            onClick={() => setDevice({ width: device.height, height: device.width })}
          >
            <ArrowsClockwise size={14} />
          </button>
          <span className="bw-strip-fill" />
          <button type="button" className="bw-btn" title="Leave device view" onClick={toggleDevice}>
            <X size={14} />
          </button>
        </div>
      )}

      <div className={`bw-page${device ? ' device' : ''}`} ref={pageRef}>
        <img ref={pictureRef} className="bw-picture" alt="" />
        {!live && (
          <div className="bw-ntp">
            <div className="bw-ntp-col">
              <form
                className="bw-ntp-search"
                onSubmit={(e) => {
                  e.preventDefault()
                  go(e.currentTarget.elements.q.value)
                }}
              >
                <MagnifyingGlass size={18} className="bw-ntp-icon" />
                <input name="q" placeholder="Search Google or type a URL" spellCheck={false} autoFocus />
              </form>
              {recent.length > 0 && (
                // The addresses this window has read, newest first: the same
                // record the history list draws, so a private tab is not here
                // (it was never recorded). A row opens in this tab.
                <section className="bw-ntp-recent" aria-label="Recent">
                  <h2 className="bw-ntp-recent-label">Recent</h2>
                  {recent.map((entry) => (
                    <button
                      key={entry.url}
                      type="button"
                      className="bw-history-row bw-ntp-recent-row"
                      title={entry.url}
                      onClick={() => openUrl(entry.url)}
                    >
                      <span className="bw-history-title">{visitLabel(entry)}</span>
                      <span className="bw-history-url">{shownVisitUrl(entry.url)}</span>
                    </button>
                  ))}
                </section>
              )}
            </div>
          </div>
        )}
      </div>

      {historyOpen && (
        <BrowserHistoryModal
          entries={history}
          onOpen={(url) => {
            setHistoryOpen(false)
            openUrl(url)
          }}
          onClear={clearHistory}
          onClose={() => setHistoryOpen(false)}
        />
      )}
      {editor && (
        <BrowserBookmarkEditor
          mark={editor.mark}
          fresh={editor.fresh}
          onDone={finishEditing}
          onRemove={removeEdited}
        />
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems()} onClose={() => setMenu(null)} />}
    </div>
  )
}
