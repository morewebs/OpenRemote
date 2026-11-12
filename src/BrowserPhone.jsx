// The built-in browser on a phone: Chrome Android's arrangement in the
// app's colours. A top bar (home, the address, the tab count, the menu)
// that slides away as the page scrolls down and comes back as it scrolls
// up, a tab grid with a Private section, the menu's pages (history,
// downloads, bookmarks, settings) and the address bar's suggestions.
//
// Each page is a native view the Android shell lays over this view's
// placeholder (src-tauri/src/mobile_browser.rs and its Kotlin plugin). A
// native view always draws above the console, so whatever of the console
// covers the page either hides it (the full-screen pages) or shows the
// page's picture in its place while the page waits out of sight (the menu,
// a link's menu).
//
// Everything it keeps is the desktop browser's: the tabs, the session, the
// sites' requests and the reading record live in browser-store.js, the
// bookmarks are the daemon's rows through the store, and the search engine
// and the sites' remembered answers are browser-prefs.js's. The view only
// draws them the phone's way.

import { useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import {
  ArrowClockwise,
  ArrowLeft,
  ArrowRight,
  ArrowSquareOut,
  BookmarkSimple,
  CaretDown,
  CaretUp,
  Check,
  ClockCounterClockwise,
  Copy,
  Desktop,
  Detective,
  DotsThreeVertical,
  DownloadSimple,
  GearSix,
  Globe,
  House,
  Image as ImageIcon,
  Link as LinkIcon,
  Lock,
  MagnifyingGlass,
  Plus,
  ShareNetwork,
  Star,
  Trash,
  Warning,
  WarningCircle,
  X,
} from '@phosphor-icons/react'
import { hasTauri } from './platform.js'
import { SEARCH_ENGINES, omnibox, searchUrl, security, suggest } from './browser-omnibox.js'
import { closeTab, isDormant, newTab, rememberClosed, tabTitle } from './browser-tabs.js'
import {
  adopt,
  answerPrompt,
  browserCall,
  call,
  closePage,
  focusTab,
  getState,
  patchTab,
  recordVisit,
  removeBookmark,
  saveBookmark,
  setState,
  startSession,
  subscribe,
} from './browser-store.js'
import { bookmarkFor, sortBookmarks } from './browser-bookmarks.js'
import { groupByDay } from './browser-history.js'
import { tileLabel, tileLetter, tilesFrom } from './browser-newtab.js'
import { browserSetting, loadBrowserPrefs, setBrowserPref, setSitePermission, useBrowserPrefs } from './browser-prefs.js'
import { fileName, permissionAsk, permissionName, permissionRows } from './browser-chrome.js'
import {
  addressLine,
  agentLine,
  contextActions,
  downloadLine,
  gridSections,
  openerTab,
  pageBounds,
  shownPage,
  siteOf,
  thumbWanted,
} from './browser-phone.js'
import BrowserClearData from './BrowserClearData.jsx'
import { errorCopy } from './error-copy.js'
import { useBack } from './use-back.js'
import './browser-phone.css'

// The tab grid's pictures, at most this many CSS pixels wide.
const THUMB = 360

// Resolves after `n` frames have been painted.
function frames(n) {
  return new Promise((resolve) => {
    const step = (left) => (left ? requestAnimationFrame(() => step(left - 1)) : resolve())
    step(n)
  })
}

// Reduced motion, either way it is asked for: the page's bar then snaps.
function still() {
  try {
    return window.matchMedia('(prefers-reduced-motion: reduce)').matches || document.documentElement.hasAttribute('data-reduce-motion')
  } catch {
    return false
  }
}

// A site's first letter, standing in for its icon (the console shows no
// images from the web).
function initial(url, title) {
  return (siteOf(url) || title || '?').charAt(0).toUpperCase()
}

function timeOf(at) {
  return new Date(at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
}

async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    // An older webview: the textarea way.
    const area = document.createElement('textarea')
    area.value = text
    area.style.position = 'fixed'
    area.style.opacity = '0'
    document.body.appendChild(area)
    area.select()
    const done = document.execCommand('copy')
    area.remove()
    return done
  }
}

// A shell command whose refusal the view says in words (the store's `call`
// answers null for one).
async function shell(command, args) {
  const { invoke } = await import('@tauri-apps/api/core')
  return invoke(command, args)
}

// A shell event while the view is up, with the newest handler.
function useShellEvent(name, handler) {
  const latest = useRef(handler)
  latest.current = handler
  useEffect(() => {
    if (!hasTauri) return undefined
    let off = null
    let gone = false
    import('@tauri-apps/api/event')
      .then(({ listen }) => listen(name, (e) => latest.current(e.payload)))
      .then((unlisten) => {
        if (gone) unlisten()
        else off = unlisten
      })
      .catch(() => {})
    return () => {
      gone = true
      off?.()
    }
  }, [name])
}

// ---- the view ----

export default function BrowserPhone({ onClose = null, askToShow = null }) {
  const { tabs, active, downloads, finds, prompts, bookmarks, bookmarksNote, history } = useSyncExternalStore(subscribe, getState)
  const prefs = useBrowserPrefs()
  const current = tabs.find((t) => t.key === active) ?? tabs[0]
  const [surface, setSurface] = useState(null)
  const [editing, setEditing] = useState(false)
  const [menu, setMenu] = useState(false)
  const [context, setContext] = useState(null)
  const [finding, setFinding] = useState(false)
  const [findTerm, setFindTerm] = useState('')
  const [clearing, setClearing] = useState(false)
  const [note, setNote] = useState(null)
  const [thumbs, setThumbs] = useState({})
  const pageRef = useRef(null)
  const barRef = useRef(null)
  const pictureRef = useRef(null)
  const shownNative = useRef(null)
  const parked = useRef(false)
  const findingRef = useRef(false)
  findingRef.current = finding
  const covered = Boolean(surface || editing || clearing)
  const coveredRef = useRef(covered)
  coveredRef.current = covered

  const engine = browserSetting(prefs.settings, 'search')
  const engineName = SEARCH_ENGINES[engine].name
  const activeTab = () => getState().tabs.find((t) => t.key === getState().active)

  // The last session comes back once per run, and the tabs are saved as they
  // change: the store's own, shared with the desktop's view.
  useEffect(() => {
    if (!hasTauri) return
    startSession()
    loadBrowserPrefs(browserCall)
  }, [])

  const noteTimer = useRef(0)
  const say = (text) => {
    clearTimeout(noteTimer.current)
    setNote(text)
    noteTimer.current = setTimeout(() => setNote(null), 2600)
  }
  useEffect(() => () => clearTimeout(noteTimer.current), [])

  // Android's back, oldest first: it leaves the browser for the chat once
  // nothing newer (a menu, a page, a popup's opener) answers it.
  useBack(Boolean(onClose), () => onClose?.())

  // ---- where the page sits ----

  const rect = () =>
    pageBounds(pageRef.current?.getBoundingClientRect(), {
      dpr: window.devicePixelRatio || 1,
      bar: barRef.current?.offsetHeight ?? 0,
      still: still(),
      finding: findingRef.current,
    })

  const syncBounds = () => {
    const tab = shownNative.current
    const bounds = rect()
    // While a menu covers the page it waits far out of view, the same size.
    if (tab && bounds) call('browser_set_bounds', { tab, bounds: parked.current ? { ...bounds, x: -30000 } : bounds })
  }

  // The page on screen: the active tab's, unless a full-screen page of the
  // console covers it or the tab shows the new tab page.
  const wanted = shownPage(current, covered)
  useLayoutEffect(() => {
    if (!hasTauri) return
    const before = shownNative.current
    shownNative.current = wanted
    if (barRef.current) barRef.current.style.transform = ''
    // Each command runs on its own in the shell, so a quick second switch
    // could overtake this one: each step goes only while it is still the
    // page wanted.
    const latest = () => shownNative.current === wanted
    ;(async () => {
      if (before && before !== wanted) await call('browser_hide', { tab: before })
      const bounds = rect()
      if (wanted && bounds && latest()) {
        await call('browser_set_bounds', { tab: wanted, bounds })
        if (latest()) await call('browser_show', { tab: wanted, focus: false })
      }
    })()
  }, [wanted])

  // Leaving the view hides the page; the tab stays.
  useEffect(
    () => () => {
      const shown = shownNative.current
      shownNative.current = null
      if (shown) call('browser_hide', { tab: shown })
    },
    [],
  )

  // The page follows the placeholder: the keyboard, a rotation, the bar's
  // own rows coming and going.
  useEffect(() => {
    if (!hasTauri) return undefined
    let frame = 0
    const sync = () => {
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(syncBounds)
    }
    const ro = new ResizeObserver(sync)
    if (pageRef.current) ro.observe(pageRef.current)
    if (barRef.current) ro.observe(barRef.current)
    window.addEventListener('resize', sync)
    return () => {
      cancelAnimationFrame(frame)
      ro.disconnect()
      window.removeEventListener('resize', sync)
    }
  }, [])

  useEffect(() => {
    syncBounds()
  }, [finding])

  // The bar slides as the page scrolls: the shell says how far, once a frame.
  useShellEvent('browser:chrome', (p) => {
    if (p.tab !== shownNative.current || !barRef.current) return
    barRef.current.style.transform = p.offset > 0 ? `translateY(${-p.offset}px)` : ''
  })

  // A long press on a link or a picture in the page.
  useShellEvent('browser:context', (p) => {
    if (p.tab === shownNative.current) setContext(p)
  })

  // The menu or a link's menu over the page: the page's picture is decoded
  // and shown first, and only then does the page step out of view; when the
  // menu closes the page steps back and the picture goes a moment later. The
  // picture lives in this element only and is dropped when the menu closes.
  const overlay = Boolean(menu || context)
  const swapped = useRef(false)
  useEffect(() => {
    if (!hasTauri) return undefined
    let stale = false
    ;(async () => {
      const picture = pictureRef.current
      if (overlay && !swapped.current) {
        const tab = shownNative.current
        if (!tab) return
        swapped.current = true
        const url = await call('browser_capture', { tab })
        if (stale) return
        if (url && picture) {
          if (picture.getAttribute('src') !== url) picture.src = url
          await picture.decode().catch(() => {})
          picture.classList.add('on')
          await frames(2)
        }
        parked.current = true
        syncBounds()
      } else if (!overlay && swapped.current) {
        swapped.current = false
        parked.current = false
        syncBounds()
        await frames(3)
        picture?.classList.remove('on')
        picture?.removeAttribute('src')
      }
    })()
    return () => {
      stale = true
    }
  }, [overlay])

  // ---- the reading record ----

  // As the desktop's view does: the page shown is a visit, the late title
  // the same visit, and a private tab is recorded nowhere.
  useEffect(() => {
    if (current?.private) return
    const url = current?.url
    if (!url || !current?.native || current.home || !/^https?:\/\//i.test(url)) return
    recordVisit({ url, title: current?.title ?? '' })
  }, [active, current?.url, current?.title, current?.private, current?.native, current?.home])

  // ---- going places ----

  // A tab with no page yet gets one at `url`; a refusal (a private tab on a
  // phone whose WebView can't keep one apart) is said in words.
  const openIn = async (tab, url) => {
    patchTab(tab.key, { url, loading: true, home: false, failed: false })
    await frames(2)
    const bounds = rect() ?? { x: 0, y: 0, width: 0, height: 0 }
    let native = null
    try {
      native = await shell('browser_open', { url, bounds, show: false, private: Boolean(tab.private) })
    } catch (err) {
      say(errorCopy(err))
    }
    if (!native || !getState().tabs.some((t) => t.key === tab.key)) {
      patchTab(tab.key, { loading: false, failed: !native })
      if (native) closePage(native)
      return
    }
    adopt(tab.key, native)
  }

  // Opens what was typed (an address, or a search) in the active tab, or a
  // new one.
  const go = async (line, { fresh = false, private: priv } = {}) => {
    const url = omnibox(line, engine)
    if (!url) return
    setEditing(false)
    setSurface(null)
    let tab = activeTab()
    if (fresh || !tab) {
      const made = newTab(priv ? { private: true } : {})
      setState((s) => ({ tabs: [...s.tabs, made], active: made.key }))
      tab = made
    }
    if (tab.native) {
      patchTab(tab.key, { home: false })
      await call('browser_navigate', { tab: tab.native, url })
      return
    }
    await openIn(tab, url)
  }
  const goRef = useRef(go)
  goRef.current = go

  // A tab waiting as an entry (brought back from the last session, or put
  // away) opens its page when it is shown, not before.
  const openInRef = useRef(openIn)
  openInRef.current = openIn
  useEffect(() => {
    if (!hasTauri || covered || !isDormant(current)) return
    openInRef.current(current, current.url)
  }, [current?.key, current?.native, current?.url, current?.loading, current?.failed, covered])

  const openNewTab = (priv) => {
    const tab = newTab(priv ? { private: true } : {})
    setState((s) => ({ tabs: [...s.tabs, tab], active: tab.key }))
    setSurface(null)
    setMenu(false)
  }

  // Closes a tab; `then` is the tab to show after (the opener, going back).
  const close = (key, then = null) => {
    const tab = getState().tabs.find((t) => t.key === key)
    if (tab?.native) closePage(tab.native)
    setState((s) => {
      const next = closeTab(s.tabs, key, s.active)
      const show = then && next.tabs.some((t) => t.key === then) ? then : next.active
      return { tabs: next.tabs, active: show, closed: rememberClosed(s.closed, next.closed) }
    })
  }

  const goHome = () => {
    const tab = activeTab()
    if (tab?.native) patchTab(tab.key, { home: true })
  }

  // The app asked for the browser: a chat's agent's tab, or an address. The
  // request is a value, so a repeat of the same one is answered once.
  const ask = useRef(null)
  useEffect(() => {
    if (!askToShow || askToShow === ask.current) return
    ask.current = askToShow
    if (askToShow.tab) {
      const shown = getState().tabs.find((t) => t.key === askToShow.tab)
      if (shown) setState(() => ({ active: shown.key }))
      else focusTab(askToShow.tab)
      return
    }
    if (askToShow.url) requestAnimationFrame(() => goRef.current?.(askToShow.url, { fresh: true }))
  }, [askToShow])

  // ---- sites' requests ----

  // Main's permission flow: a remembered answer was given already, so a
  // request waiting here is one the user has not answered; a private tab's
  // answer is never remembered.
  const prompt = prompts.find((p) => p.tab === current?.native)
  const answer = (allow, remember) => {
    if (!prompt) return
    answerPrompt(prompt, allow, remember).catch((err) => say(errorCopy(err)))
  }

  // ---- find in page ----

  const runFind = (term, step) => {
    const native = activeTab()?.native
    if (native) call('browser_find', { tab: native, term, step })
  }

  const openFind = () => {
    setMenu(false)
    if (!activeTab()?.native) return
    setFindTerm('')
    setFinding(true)
  }

  const closeFind = () => {
    setFinding(false)
    setFindTerm('')
    const native = activeTab()?.native
    if (native) call('browser_find', { tab: native, term: '', step: 0 })
  }

  // ---- back ----

  // Android's back goes to the page's own history first (the shell answers
  // it while the page is plainly in view), then here: whatever is open,
  // then a tab another tab opened goes back to its opener; then the
  // browser closes back to the chat.
  const opener = openerTab(tabs, current)
  useBack(Boolean(surface), () => setSurface(null))
  useBack(editing, () => setEditing(false))
  useBack(menu, () => setMenu(false))
  useBack(Boolean(context), () => setContext(null))
  useBack(finding, closeFind)
  useBack(Boolean(current?.home), () => {
    const tab = activeTab()
    if (tab) patchTab(tab.key, { home: false })
  })
  useBack(Boolean(opener) && !covered && !finding && !menu && !context && !current?.home, () => {
    const s = getState()
    const tab = s.tabs.find((t) => t.key === s.active)
    const back = openerTab(s.tabs, tab)
    if (tab && back) close(tab.key, back.key)
  })

  // ---- the tab grid ----

  const picture = async (tab) => {
    if (!thumbWanted(tab)) return
    const url = await call('browser_capture', { tab: tab.native, max: THUMB })
    if (url) setThumbs((m) => ({ ...m, [tab.native]: url }))
  }

  const openGrid = async () => {
    // The page on screen is pictured before the grid hides it.
    const shown = activeTab()
    if (shown?.native && shown.native === shownNative.current) await picture(shown)
    setSurface('tabs')
  }

  useEffect(() => {
    if (surface !== 'tabs' || !hasTauri) return undefined
    let stale = false
    ;(async () => {
      for (const t of getState().tabs) {
        if (stale) return
        if (!thumbWanted(t)) continue
        const url = await call('browser_capture', { tab: t.native, max: THUMB })
        if (url && !stale) setThumbs((m) => ({ ...m, [t.native]: url }))
      }
    })()
    return () => {
      stale = true
    }
  }, [surface])

  // Pictures of tabs that closed go with them.
  useEffect(() => {
    setThumbs((m) => {
      const open = new Set(tabs.map((t) => t.native).filter(Boolean))
      const kept = Object.fromEntries(Object.entries(m).filter(([native]) => open.has(native)))
      return Object.keys(kept).length === Object.keys(m).length ? m : kept
    })
  }, [tabs])

  // ---- what the menus do ----

  const live = Boolean(current?.native) && !current?.home
  const bookmark = bookmarkFor(bookmarks, current?.url)

  const toggleBookmark = () => {
    setMenu(false)
    if (!live || !current?.url) return
    if (bookmark) removeBookmark(bookmark.id)
    else saveBookmark({ url: current.url, title: current.title ?? '' })
    say(bookmark ? 'Bookmark removed' : 'Bookmarked')
  }

  const control = async (action) => {
    if (!current?.native) return
    try {
      await browserCall('POST', `/browser/tabs/${encodeURIComponent(current.native)}/control`, { action })
    } catch (err) {
      say(errorCopy(err))
    }
  }

  // The phone's share sheet. Handing a private tab to agents is the
  // desktop's `browser_share`, in the menu below.
  const shareSheet = (args) => {
    shell('browser_share_sheet', args).catch((err) => say(errorCopy(err)))
  }

  const contextAction = async (what) => {
    const c = context
    setContext(null)
    if (!c) return
    const target = c.link ?? c.url
    if (what === 'open') go(target, { fresh: true, private: Boolean(current?.private) })
    else if (what === 'private') go(target, { fresh: true, private: true })
    else if (what === 'image') go(c.url, { fresh: true, private: Boolean(current?.private) })
    else if (what === 'copy') say((await copyText(target)) ? 'Link copied' : "The link couldn't be copied")
    else if (what === 'copy-image') say((await copyText(c.url)) ? 'Picture address copied' : "The address couldn't be copied")
    else if (what === 'share') shareSheet({ url: target })
    else if (what === 'download') {
      try {
        await shell('browser_download', { tab: c.tab, url: c.kind === 'image' ? c.url : target })
        say('Downloading')
      } catch (err) {
        say(errorCopy(err))
      }
    }
  }

  const CONTEXT_ITEMS = {
    open: [<Plus size={18} />, 'Open in new tab'],
    private: [<Detective size={18} />, 'Open in private tab'],
    copy: [<Copy size={18} />, 'Copy link address'],
    share: [<ShareNetwork size={18} />, 'Share link'],
    image: [<ImageIcon size={18} />, 'Open picture in new tab'],
    download: [<DownloadSimple size={18} />, context?.kind === 'image' ? 'Download picture' : 'Download link'],
    'copy-image': [<Copy size={18} />, 'Copy picture address'],
  }

  // ---- drawing ----

  const findState = (current?.native && finds[current.native]) || null
  const lock = security(current?.url)
  const sameMode = tabs.filter((t) => Boolean(t.private) === Boolean(current?.private))
  const controller = live ? current?.controller : null
  const loading = live && current?.loading

  return (
    <div className={`bp${current?.private ? ' private' : ''}`}>
      <header className="bp-bar" ref={barRef}>
        {finding ? (
          <div className="bp-top bp-find">
            <MagnifyingGlass size={18} className="bp-find-icon" />
            <input
              className="bp-find-input"
              value={findTerm}
              placeholder="Find in page"
              spellCheck={false}
              autoFocus
              enterKeyHint="search"
              onChange={(e) => {
                setFindTerm(e.target.value)
                runFind(e.target.value, 0)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault()
                  runFind(findTerm, 1)
                }
              }}
            />
            <span className="bp-find-count">
              {findTerm ? (findState?.count ? `${findState.active}/${findState.count}` : '0/0') : ''}
            </span>
            <button type="button" className="bp-icon" aria-label="Previous match" onClick={() => runFind(findTerm, -1)}>
              <CaretUp size={20} />
            </button>
            <button type="button" className="bp-icon" aria-label="Next match" onClick={() => runFind(findTerm, 1)}>
              <CaretDown size={20} />
            </button>
            <button type="button" className="bp-icon" aria-label="Close find in page" onClick={closeFind}>
              <X size={20} />
            </button>
          </div>
        ) : (
          <div className="bp-top">
            <button type="button" className="bp-icon" aria-label="Home" disabled={!live} onClick={goHome}>
              <House size={22} />
            </button>
            <button type="button" className="bp-address" onClick={() => setEditing(true)}>
              {current?.private && <Detective size={17} className="bp-address-private" />}
              {live &&
                (lock === 'insecure' ? (
                  <Warning size={16} className="bp-address-warn" />
                ) : lock === 'none' ? (
                  <Globe size={16} className="bp-address-lock" />
                ) : (
                  <Lock size={16} className="bp-address-lock" />
                ))}
              <span className={`bp-address-text pv${live ? '' : ' quiet'}`}>{addressLine(current)}</span>
            </button>
            <button type="button" className="bp-count" aria-label={`${sameMode.length} open tabs`} onClick={openGrid}>
              <span>{sameMode.length > 99 ? '99+' : sameMode.length}</span>
            </button>
            <button type="button" className="bp-icon" aria-label="More" onClick={() => setMenu(true)}>
              <DotsThreeVertical size={22} weight="bold" />
            </button>
          </div>
        )}
        {loading && <div className="bp-progress" style={{ width: `${Math.max(8, current.progress ?? 10)}%` }} />}
        {controller && (
          <div className="bp-agent">
            <span className={`bp-agent-dot${controller.kind === 'paused' ? ' paused' : ''}`} />
            <span className="bp-agent-text pv">{agentLine(controller)}</span>
            <button type="button" className="bp-agent-btn" onClick={() => control(controller.kind === 'paused' ? 'hand_back' : 'take_over')}>
              {controller.kind === 'paused' ? 'Hand back' : 'Take over'}
            </button>
          </div>
        )}
      </header>

      {live && current.problem && (
        <div className="bp-infobar">
          <WarningCircle size={18} className="bp-infobar-icon" />
          <span>{current.problem}</span>
        </div>
      )}

      <div className="bp-page" ref={pageRef}>
        <img ref={pictureRef} className="bp-picture" alt="" />
        {(!current?.native || current.home) && (
          <NewTabPage
            engineName={engineName}
            isPrivate={Boolean(current?.private)}
            onGo={(line) => go(line)}
            onSearch={() => setEditing(true)}
          />
        )}
        {current?.native && !current.home && current.crashed && (
          <div className="bp-crash">
            <WarningCircle size={30} />
            <p className="bp-crash-title">This page stopped working</p>
            <p className="bp-crash-text">Its content was too much for the phone, or it crashed. Reloading starts it again.</p>
            <button type="button" className="bp-button primary" onClick={() => call('browser_reload', { tab: current.native })}>
              Reload
            </button>
          </div>
        )}
      </div>

      {prompt && (
        <div className="bp-ask" role="alert">
          <p className="bp-ask-text">
            <strong className="pv">{siteOf(prompt.origin) || prompt.origin}</strong> wants to {permissionAsk(prompt.kind, prompt.label)}
          </p>
          <div className="bp-ask-actions">
            {/* A private tab's answer is for this request only. */}
            <button type="button" className="bp-button" onClick={() => answer(false, !prompt.private)}>
              Block
            </button>
            <button type="button" className="bp-button primary" onClick={() => answer(true, !prompt.private)}>
              Allow
            </button>
          </div>
        </div>
      )}

      {note && <div className="bp-note">{note}</div>}

      {editing && (
        <Omnibox
          engine={engine}
          bookmarks={bookmarks}
          history={history}
          current={live ? current : null}
          onGo={(line) => go(line)}
          onClose={() => setEditing(false)}
          onCopied={(ok) => say(ok ? 'Link copied' : "The link couldn't be copied")}
          onShare={() => {
            setEditing(false)
            if (live) shareSheet({ tab: current.native, title: current.title })
          }}
        />
      )}

      {surface === 'tabs' && (
        <TabGrid
          tabs={tabs}
          active={active}
          thumbs={thumbs}
          onPick={(key) => {
            setState(() => ({ active: key }))
            setSurface(null)
          }}
          onClose={(key) => close(key)}
          onNew={openNewTab}
          onDone={() => setSurface(null)}
        />
      )}
      {surface === 'history' && (
        <HistoryPage history={history} onOpen={(url) => go(url)} onClose={() => setSurface(null)} onClearData={() => setClearing(true)} />
      )}
      {surface === 'downloads' && <DownloadsPage downloads={downloads} onClose={() => setSurface(null)} onSay={say} />}
      {surface === 'bookmarks' && (
        <BookmarksPage bookmarks={bookmarks} note={bookmarksNote} onOpen={(url) => go(url)} onClose={() => setSurface(null)} />
      )}
      {surface === 'settings' && (
        <SettingsPage prefs={prefs} engine={engine} onSay={say} onClearData={() => setClearing(true)} onClose={() => setSurface(null)} />
      )}
      {clearing && <BrowserClearData onClose={() => setClearing(false)} onCleared={() => say('Browsing data cleared')} />}

      {menu && (
        <div className="bp-menu-backdrop" onClick={() => setMenu(false)}>
          <div className="bp-menu" role="menu" onClick={(e) => e.stopPropagation()}>
            <div className="bp-menu-top">
              <button
                type="button"
                className="bp-icon"
                aria-label="Forward"
                disabled={!live || !current.canForward}
                onClick={() => {
                  setMenu(false)
                  call('browser_forward', { tab: current.native })
                }}
              >
                <ArrowRight size={20} />
              </button>
              <button type="button" className="bp-icon" aria-label={bookmark ? 'Remove bookmark' : 'Bookmark this page'} disabled={!live} onClick={toggleBookmark}>
                <Star size={20} weight={bookmark ? 'fill' : 'regular'} className={bookmark ? 'bp-starred' : ''} />
              </button>
              <button
                type="button"
                className="bp-icon"
                aria-label={loading ? 'Stop loading' : 'Reload'}
                disabled={!live}
                onClick={() => {
                  setMenu(false)
                  call(loading ? 'browser_stop' : 'browser_reload', { tab: current.native })
                }}
              >
                {loading ? <X size={20} /> : <ArrowClockwise size={20} />}
              </button>
            </div>
            <MenuItem icon={<Plus size={18} />} label="New tab" onSelect={() => openNewTab(false)} />
            <MenuItem icon={<Detective size={18} />} label="New private tab" onSelect={() => openNewTab(true)} />
            <div className="bp-menu-sep" />
            <MenuItem
              icon={<ClockCounterClockwise size={18} />}
              label="History"
              onSelect={() => {
                setMenu(false)
                setSurface('history')
              }}
            />
            <MenuItem
              icon={<DownloadSimple size={18} />}
              label="Downloads"
              hint={downloads.some((d) => d.state === 'started') ? 'Downloading' : ''}
              onSelect={() => {
                setMenu(false)
                setSurface('downloads')
              }}
            />
            <MenuItem
              icon={<BookmarkSimple size={18} />}
              label="Bookmarks"
              onSelect={() => {
                setMenu(false)
                setSurface('bookmarks')
              }}
            />
            <div className="bp-menu-sep" />
            <MenuItem
              icon={<ShareNetwork size={18} />}
              label="Share"
              disabled={!live}
              onSelect={() => {
                setMenu(false)
                if (live) shareSheet({ tab: current.native, title: current.title })
              }}
            />
            <MenuItem icon={<MagnifyingGlass size={18} />} label="Find in page" disabled={!live} onSelect={openFind} />
            <MenuItem
              icon={<Desktop size={18} />}
              label="Desktop site"
              checked={Boolean(current?.desktop)}
              disabled={!live}
              onSelect={() => {
                setMenu(false)
                call('browser_desktop_site', { tab: current.native, on: !current.desktop })
              }}
            />
            {current?.private && (
              <MenuItem
                icon={<Detective size={18} />}
                label="Let agents use this tab"
                checked={Boolean(current.shared)}
                disabled={!live}
                onSelect={() => {
                  setMenu(false)
                  call('browser_share', { tab: current.native, on: !current.shared })
                }}
              />
            )}
            <MenuItem
              icon={<ArrowSquareOut size={18} />}
              label="Open in your default browser"
              disabled={!live || !/^https?:/.test(current?.url ?? '')}
              onSelect={() => {
                setMenu(false)
                shell('browser_open_external', { url: current.url }).catch((err) => say(errorCopy(err)))
              }}
            />
            <div className="bp-menu-sep" />
            <MenuItem
              icon={<GearSix size={18} />}
              label="Settings"
              onSelect={() => {
                setMenu(false)
                setSurface('settings')
              }}
            />
          </div>
        </div>
      )}

      {context && (
        <div className="bp-sheet-backdrop" onClick={() => setContext(null)}>
          <div className="bp-sheet" role="menu" onClick={(e) => e.stopPropagation()}>
            <div className="bp-sheet-head">
              {context.kind === 'image' ? <ImageIcon size={18} /> : <LinkIcon size={18} />}
              <span className="bp-sheet-url pv">{context.link ?? context.url}</span>
            </div>
            {contextActions(context, Boolean(current?.private)).map((what) => (
              <MenuItem key={what} icon={CONTEXT_ITEMS[what][0]} label={CONTEXT_ITEMS[what][1]} onSelect={() => contextAction(what)} />
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function MenuItem({ icon, label, hint, checked, disabled, onSelect }) {
  return (
    <button type="button" role="menuitem" className="bp-menu-item" disabled={disabled} onClick={onSelect}>
      <span className="bp-menu-icon">{icon}</span>
      <span className="bp-menu-label">{label}</span>
      {hint && <span className="bp-menu-hint">{hint}</span>}
      {checked !== undefined && (
        <span className={`bp-check${checked ? ' on' : ''}`} aria-hidden="true">
          {checked && <Check size={13} weight="bold" />}
        </span>
      )}
    </button>
  )
}

// ---- the new tab page ----

function NewTabPage({ engineName, isPrivate, onGo, onSearch }) {
  const [tiles, setTiles] = useState([])
  useEffect(() => {
    if (isPrivate) return undefined
    let stale = false
    browserCall('GET', '/browser/tiles')
      .then((answer) => {
        if (!stale) setTiles(tilesFrom(answer))
      })
      .catch(() => {})
    return () => {
      stale = true
    }
  }, [isPrivate])

  return (
    <div className="bp-ntp">
      {isPrivate ? (
        <div className="bp-private-note">
          <Detective size={34} />
          <p className="bp-private-title">Private tab</p>
          <p>
            Pages you open here stay out of your history, leave no picture behind, and agents can't see this tab unless you let them from
            the menu. Its cookies and site data go when the last private tab closes.
          </p>
        </div>
      ) : null}
      <button type="button" className="bp-ntp-search" onClick={onSearch}>
        <MagnifyingGlass size={18} />
        <span>Search {engineName} or type a web address</span>
      </button>
      {!isPrivate && tiles.length > 0 && (
        <div className="bp-tiles">
          {tiles.map((t) => (
            <button key={t.url} type="button" className="bp-tile" onClick={() => onGo(t.url)}>
              <span className="bp-tile-letter">{tileLetter(t)}</span>
              <span className="bp-tile-name pv">{tileLabel(t)}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

// ---- the address bar, open ----

function Omnibox({ engine, bookmarks, history, current, onGo, onClose, onCopied, onShare }) {
  const [text, setText] = useState(current?.url ?? '')
  const inputRef = useRef(null)

  useEffect(() => {
    const input = inputRef.current
    if (!input) return
    input.focus()
    input.select()
  }, [])

  const typed = text.trim()
  // The history's and bookmarks' rows that match, from this window's own
  // record (the same rows the desktop's address bar offers).
  const suggestions = useMemo(
    () => (typed && typed !== current?.url ? suggest({ typed, marks: bookmarks, visits: history }) : []),
    [typed, bookmarks, history, current?.url],
  )
  const target = typed ? omnibox(typed, engine) : null
  const isSearch = Boolean(target) && target === searchUrl(typed, engine)

  return (
    <div className="bp-surface bp-omnibox">
      <form
        className="bp-omnibox-bar"
        onSubmit={(e) => {
          e.preventDefault()
          if (typed) onGo(typed)
        }}
      >
        <button type="button" className="bp-icon" aria-label="Back" onClick={onClose}>
          <ArrowLeft size={22} />
        </button>
        <input
          ref={inputRef}
          className="bp-omnibox-input"
          value={text}
          placeholder="Search or type a web address"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          enterKeyHint="go"
          inputMode="url"
          onChange={(e) => setText(e.target.value)}
        />
        {text && (
          <button type="button" className="bp-icon" aria-label="Clear" onClick={() => setText('')}>
            <X size={20} />
          </button>
        )}
      </form>
      <div className="bp-surface-body">
        {current && typed === current.url && (
          <div className="bp-current">
            <span className="bp-row-letter">{initial(current.url, current.title)}</span>
            <span className="bp-row-main">
              <span className="bp-row-title pv">{current.title || siteOf(current.url)}</span>
              <span className="bp-row-sub pv">{current.url}</span>
            </span>
            <button type="button" className="bp-icon" aria-label="Copy link" onClick={async () => onCopied(await copyText(current.url))}>
              <Copy size={19} />
            </button>
            <button type="button" className="bp-icon" aria-label="Share" onClick={onShare}>
              <ShareNetwork size={19} />
            </button>
          </div>
        )}
        {target && typed !== current?.url && (
          <button type="button" className="bp-row" onClick={() => onGo(typed)}>
            <span className="bp-row-letter">{isSearch ? <MagnifyingGlass size={16} /> : <Globe size={16} />}</span>
            <span className="bp-row-main">
              <span className="bp-row-title pv">{typed}</span>
              <span className="bp-row-sub pv">{isSearch ? `${SEARCH_ENGINES[engine].name} search` : target}</span>
            </span>
          </button>
        )}
        {suggestions.map((s) => (
          <button key={s.url} type="button" className="bp-row" onClick={() => onGo(s.url)}>
            <span className="bp-row-letter">{s.kept ? <Star size={16} weight="fill" /> : initial(s.url, s.title)}</span>
            <span className="bp-row-main">
              <span className="bp-row-title pv">{s.title || siteOf(s.url)}</span>
              <span className="bp-row-sub pv">{s.url}</span>
            </span>
          </button>
        ))}
      </div>
    </div>
  )
}

// ---- the tab grid ----

function TabGrid({ tabs, active, thumbs, onPick, onClose, onNew, onDone }) {
  const { normal, kept } = gridSections(tabs)
  const card = (t) => {
    // A private tab is never pictured: its card shows its letter.
    const shot = thumbWanted(t) ? thumbs[t.native] : null
    return (
      <div
        key={t.key}
        role="button"
        tabIndex={0}
        className={`bp-card${t.key === active ? ' on' : ''}`}
        onClick={() => onPick(t.key)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') onPick(t.key)
        }}
      >
        <div className="bp-card-head">
          {t.favicon ? <img className="bp-card-icon" src={t.favicon} alt="" /> : <Globe size={14} className="bp-card-globe" />}
          <span className="bp-card-title pv">{tabTitle(t)}</span>
          <button
            type="button"
            className="bp-card-close"
            aria-label="Close tab"
            onClick={(e) => {
              e.stopPropagation()
              onClose(t.key)
            }}
          >
            <X size={14} weight="bold" />
          </button>
        </div>
        <div className="bp-card-shot">
          {shot ? (
            <img src={shot} alt="" />
          ) : (
            <span className="bp-card-blank">{t.url ? initial(t.url, t.title) : <MagnifyingGlass size={22} />}</span>
          )}
          {t.controller && <span className="bp-card-agent" title={agentLine(t.controller)} />}
        </div>
      </div>
    )
  }
  return (
    <div className="bp-surface">
      <header className="bp-surface-head">
        <span className="bp-surface-title">
          {normal.length} {normal.length === 1 ? 'tab' : 'tabs'}
        </span>
        <button type="button" className="bp-icon" aria-label="New tab" onClick={() => onNew(false)}>
          <Plus size={22} />
        </button>
        <button type="button" className="bp-done" onClick={onDone}>
          Done
        </button>
      </header>
      <div className="bp-surface-body">
        <div className="bp-grid">{normal.map(card)}</div>
        <div className="bp-section">
          <Detective size={16} />
          <span>Private</span>
          <button type="button" className="bp-section-add" onClick={() => onNew(true)}>
            <Plus size={15} />
            New private tab
          </button>
        </div>
        {kept.length > 0 ? (
          <div className="bp-grid">{kept.map(card)}</div>
        ) : (
          <p className="bp-quiet">Private tabs keep no history. Agents can't see them unless you allow it from the menu.</p>
        )}
      </div>
    </div>
  )
}

// ---- the menu's pages ----

function Page({ title, onClose, children, action }) {
  return (
    <div className="bp-surface">
      <header className="bp-surface-head">
        <button type="button" className="bp-icon" aria-label="Back" onClick={onClose}>
          <ArrowLeft size={22} />
        </button>
        <span className="bp-surface-title">{title}</span>
        {action}
      </header>
      <div className="bp-surface-body">{children}</div>
    </div>
  )
}

// This window's reading record (browser-store's), as the desktop's history
// dialog reads it: newest first, by day.
function HistoryPage({ history, onOpen, onClose, onClearData }) {
  const [query, setQuery] = useState('')
  const needle = query.trim().toLowerCase()
  const rows = needle
    ? history.filter((v) => String(v.title ?? '').toLowerCase().includes(needle) || String(v.url).toLowerCase().includes(needle))
    : history
  const groups = groupByDay(rows)

  return (
    <Page title="History" onClose={onClose}>
      <label className="bp-search">
        <MagnifyingGlass size={17} />
        <input value={query} placeholder="Search your history" spellCheck={false} onChange={(e) => setQuery(e.target.value)} />
      </label>
      <button type="button" className="bp-link-row" onClick={onClearData}>
        Clear browsing data
      </button>
      {!rows.length && <p className="bp-quiet">{needle ? 'Nothing in your history matches.' : 'Pages you visit show here.'}</p>}
      {groups.map((g) => (
        <section key={g.label}>
          <h3 className="bp-group">{g.label}</h3>
          {g.rows.map((v) => (
            <div key={`${v.url}|${v.at}`} className="bp-row">
              <button type="button" className="bp-row-open" onClick={() => onOpen(v.url)}>
                <span className="bp-row-letter">{initial(v.url, v.title)}</span>
                <span className="bp-row-main">
                  <span className="bp-row-title pv">{v.title || siteOf(v.url)}</span>
                  <span className="bp-row-sub pv">
                    {timeOf(v.at)} · {siteOf(v.url)}
                  </span>
                </span>
              </button>
            </div>
          ))}
        </section>
      ))}
    </Page>
  )
}

function DownloadsPage({ downloads, onClose, onSay }) {
  return (
    <Page title="Downloads" onClose={onClose}>
      {!downloads.length && <p className="bp-quiet">Files you download land in the phone's Downloads, and show here.</p>}
      {downloads.map((d) => (
        <button
          key={d.id}
          type="button"
          className="bp-row"
          disabled={d.state !== 'done' || !d.path}
          onClick={() => shell('browser_reveal', { path: d.path }).catch((err) => onSay(errorCopy(err)))}
        >
          <span className="bp-row-letter">{d.state === 'failed' ? <Warning size={16} /> : <DownloadSimple size={16} />}</span>
          <span className="bp-row-main">
            <span className="bp-row-title pv">{d.name ?? fileName(d.path ?? d.url)}</span>
            <span className="bp-row-sub">{downloadLine(d)}</span>
          </span>
        </button>
      ))}
    </Page>
  )
}

// The store's bookmark rows: the daemon keeps them, and a refusal puts the
// list back with a note saying it is the last one this window saw.
function BookmarksPage({ bookmarks, note, onOpen, onClose }) {
  const sorted = sortBookmarks(bookmarks)
  return (
    <Page title="Bookmarks" onClose={onClose}>
      {note && <p className="bp-quiet">{note}</p>}
      {!sorted.length && <p className="bp-quiet">Bookmark a page with the star in the menu.</p>}
      {sorted.map((b) => (
        <div key={b.id} className="bp-row">
          <button type="button" className="bp-row-open" onClick={() => onOpen(b.url)}>
            <span className="bp-row-letter">{initial(b.url, b.title)}</span>
            <span className="bp-row-main">
              <span className="bp-row-title pv">{b.title || siteOf(b.url)}</span>
              <span className="bp-row-sub pv">{b.url}</span>
            </span>
          </button>
          <button type="button" className="bp-icon" aria-label="Remove bookmark" onClick={() => removeBookmark(b.id)}>
            <Trash size={18} />
          </button>
        </div>
      ))}
    </Page>
  )
}

// The browser's settings on the phone: the search engine and the sites'
// remembered answers (browser-prefs.js, shared with Settings > Browser on the
// desktop), and Clear browsing data (the desktop's dialog).
function SettingsPage({ prefs, engine, onSay, onClearData, onClose }) {
  const remembered = permissionRows(prefs.permissions)

  const setEngine = (key) => {
    setBrowserPref(browserCall, 'search', key).catch((err) => onSay(errorCopy(err)))
  }

  const forget = (p) => {
    setSitePermission(browserCall, p.origin, p.kind, null).catch((err) => onSay(errorCopy(err)))
  }

  return (
    <Page title="Browser settings" onClose={onClose}>
      <h3 className="bp-group">Search engine</h3>
      {Object.entries(SEARCH_ENGINES).map(([key, e]) => (
        <button key={key} type="button" className="bp-row" onClick={() => setEngine(key)}>
          <span className="bp-row-main">
            <span className="bp-row-title">{e.name}</span>
          </span>
          <span className={`bp-radio${engine === key ? ' on' : ''}`} aria-hidden="true" />
        </button>
      ))}

      <h3 className="bp-group">Site permissions</h3>
      {!remembered.length && <p className="bp-quiet">Sites you allow or block (camera, microphone, location) show here.</p>}
      {remembered.map((p) => (
        <div key={`${p.origin}|${p.kind}`} className="bp-row">
          <span className="bp-row-main">
            <span className="bp-row-title pv">{siteOf(p.origin) || p.origin}</span>
            <span className="bp-row-sub">
              {permissionName(p.kind)} · {p.state === 'allow' ? 'Allowed' : 'Blocked'}
            </span>
          </span>
          <button type="button" className="bp-link" onClick={() => forget(p)}>
            Reset
          </button>
        </div>
      ))}

      <h3 className="bp-group">Privacy</h3>
      <button type="button" className="bp-link-row" onClick={onClearData}>
        Clear browsing data
      </button>
    </Page>
  )
}
