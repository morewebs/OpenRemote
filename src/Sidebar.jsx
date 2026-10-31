import { useEffect, useMemo, useRef, useState } from 'react'
import {
  Archive,
  DotsThree,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CaretDown,
  ChartBar,
  ChatCircle,
  CheckCircle,
  Circle,
  ClipboardText,
  Cloud,
  Copy,
  Desktop,
  DeviceMobile,
  FolderSimple,
  Gear,
  House,
  PushPin,
  PushPinSlash,
  Lightning,
  MagnifyingGlass,
  Globe,
  PencilSimple,
  Plus,
  PuzzlePiece,
  TrashSimple,
  UserCircle,
  X,
} from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { useConsole } from './state/console.jsx'
import { chatStatus } from './state/reducer.js'
import { attentionTitle, needsYou } from './needs-you.js'
import { duplicatedNames, folderless, projectIndex, projectLine, projectName, sections, unreadProjects, waitingProjects } from './projects.js'
import { activeSessions } from './archive.js'
import { deviceName, isPhone, sessionsForMode } from './cloud.js'
import { isMobile } from './platform.js'
import { CHAT_DRAG_TYPE, chatDragPayload } from './split.js'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { listHasUnread, loadSeen, markSeen, recordSeen, seedSeen, storeSeen, unreadIds } from './unread.js'
import { lineFor, missingLines, pruneLines, questionKey, questionLine, rememberLine, wantedKeys } from './question-line.js'
import { HarnessMark } from './brand-marks.jsx'
import ContextMenu from './ContextMenu.jsx'
import DeleteChatModal from './DeleteChatModal.jsx'
import { shownTitle } from './rename-chat.js'
import { chatTitle } from './shape.js'
import { RECONNECTING } from './pane-header.js'
import { canCopyChat, chatMarkdown, copiedLine } from './chat-markdown.js'
import { errorCopy } from './error-copy.js'
import { CHATS_PAGE, PROJECT_PAGE, loadPins, planList, storePins, togglePin } from './sidebar-list.js'
import { usePrivacyMode } from './privacy.js'
import { useHint, useShortcut } from './keyboard.jsx'
import './sidebar.css'

// How long the archived line stays. Long enough to catch a mispress on a
// hover control, short enough that the sidebar keeps no history of its own.
const UNDO_MS = 8000

// How long the copy's line stays. Shorter than the archive's: it reports what
// just happened rather than offering a way back from it.
const COPY_NOTE_MS = 4000

// No overlay, as one stable value: a fresh object here would rebuild the rows
// memo on every render of a sidebar with no rename in flight.
const NO_TITLES = {}

// Which projects the person has folded away. The sidebar's own view state,
// stored on this computer beside the seen map: folding a project is a
// preference about a place, not something the daemon should carry. Ids are the
// key, so a renamed project stays folded; the map is pruned against the
// projects the daemon answers, so an id that no longer exists is forgotten
// instead of folding a future project that happens to reuse it.
const COLLAPSED_KEY = 'openremote-collapsed-projects'

function loadCollapsed() {
  try {
    const parsed = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '{}')
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {}
    const out = {}
    for (const [id, value] of Object.entries(parsed)) if (value === true) out[id] = true
    return out
  } catch {
    return {}
  }
}

function storeCollapsed(map) {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(map))
  } catch {
    /* storage unavailable: the arrangement lasts this run */
  }
}

export default function Sidebar({
  open,
  active,
  mode,
  onMode,
  onSelect,
  onSplit,
  onAddProject,
  onNewChatIn,
  onReveal,
  // The browser's pane: the tile in this column asks for it, and the shell owns
  // whether it is open (the pane sits beside the chat, not inside it).
  onBrowser = null,
  browserOpen = false,
  // The shell hosts the rename dialog above both surfaces that offer it, so
  // the row menu only asks for it. `renamedTitles` is the one-beat overlay of
  // names just saved, until the console's own record carries them.
  onRenameChat = null,
  renamedTitles = NO_TITLES,
}) {
  const { connection, sessions, chats, devices, thisDevice, projects, projectsAnswered, sessionsAnswered, removeProject, archived, setChatArchived, deleteChat, api, reconnecting } = useConsole()
  const privacy = usePrivacyMode()
  const [query, setQuery] = useState('')
  const [searchOn, setSearchOn] = useState(false)
  const [collapsed, setCollapsed] = useState(loadCollapsed)
  // The project waiting on a "unregister?" answer. The sidebar's trash sits
  // two pixels from the project's new-chat button, and unregistering has no
  // undo, so the first press arms the row and the second acts. row-confirm.js
  // owns the rule (one armed row at a time, no id, no arming) so this surface
  // and the panes that remove plugins cannot drift apart.
  const [armed, setArmed] = useState(null)
  const [menu, setMenu] = useState(null)
  // The chat whose move is being picked for, and where the picker opens: the
  // row menu is gone by then, so its press position is the anchor.
  const [moving, setMoving] = useState(null)
  const [deleting, setDeleting] = useState(null)
  // The archive's way back. Archive is a hover button at the row's edge, so a
  // mispress otherwise costs a hunt through Settings > Archived chats that a
  // first-time user does not know exists. One line, for a moment after the
  // act, replaced by a second archive rather than stacked - the same
  // one-at-a-time rule the armed rows follow. Not the notice stack: that
  // lives in the shell, and this is the list the control lives in.
  const [undo, setUndo] = useState(null)

  // The line outlives nothing: eight seconds, or the next archive, or the
  // sidebar going away. A row that lingered would teach people to ignore it.
  useEffect(() => {
    if (!undo) return
    const timer = setTimeout(() => setUndo(null), UNDO_MS)
    return () => clearTimeout(timer)
  }, [undo])

  // The copy's own transient line: the same rule as the archive's, a little
  // shorter, because it reports a thing that already happened rather than
  // offering a way back from one.
  const [copyNote, setCopyNote] = useState(null)
  useEffect(() => {
    if (!copyNote) return
    const timer = setTimeout(() => setCopyNote(null), COPY_NOTE_MS)
    return () => clearTimeout(timer)
  }, [copyNote])
  // What this console has looked at, per chat: the stamp each chat showed
  // the last time it was read. Read once at mount from this computer's own
  // key, never on the boot path, so a damaged record cannot take the first
  // paint. The ref carries the newest value to the effects below, so the two
  // of them cannot overwrite each other's fresh write with a stale copy.
  const [seen, setSeen] = useState(() => loadSeen())
  const seenRef = useRef(seen)
  seenRef.current = seen
  const remember = (next) => {
    if (next === seenRef.current) return
    seenRef.current = next
    setSeen(next)
    storeSeen(next)
  }
  // The chats this person keeps at the top of their lists. Stored on this
  // computer beside the seen map and the folded projects: an arrangement of
  // a place, not something the daemon carries. Newest pin first, and a pin
  // whose chat has gone is simply not drawn (the rule skips it, so a deleted
  // chat cannot leave an empty row behind).
  const [pins, setPins] = useState(() => loadPins())
  const pinChat = (id) =>
    setPins((current) => {
      const next = togglePin(current, id)
      if (next !== current) storePins(next)
      return next
    })
  // Which lists have been opened up past their page for this session: the
  // list grows, the stored order does not.
  const [showAll, setShowAll] = useState(() => new Set())
  const showMore = (key) => setShowAll((current) => new Set(current).add(key))

  // Local: everything that runs on this computer. Cloud: every synced chat,
  // named by the device it runs on when that isn't this one.
  const rows = useMemo(() => {
    return sessionsForMode(activeSessions(sessions, archived), mode, thisDevice).map((session) => {
      const elsewhere = session.executor && session.executor !== thisDevice
      const device = elsewhere ? (devices ?? []).find((d) => d.id === session.executor) : null
      // A chat on screen has a live status from its own stream; every chat has
      // the poll's. They are kept apart because the dot shows the live one and
      // the waiting rule needs to know which it is reading (needs-you.js).
      const liveStatus = chats[session.id]?.status ?? null
      return {
        id: session.id,
        title: shownTitle(
          renamedTitles[session.id],
          // The live stream's title when it has one, else the poll's, and the
          // console's one word for "nothing yet" (shape.js) when neither does.
          chatTitle({ title: chats[session.id]?.title ?? session.title }),
        ),
        status: liveStatus ?? 'idle',
        liveStatus,
        polledStatus: chatStatus(session.status),
        harness: session.harness,
        workspace: session.workspace ?? null,
        updatedAt: session.updated_at ?? 0,
        device: elsewhere ? deviceName(devices, session.executor) : null,
        // A copy's last status is stale while its device is away.
        offline: elsewhere && !device?.online,
        synced: Boolean(session.executor),
      }
    })
  }, [sessions, archived, chats, mode, devices, thisDevice, renamedTitles])

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    const sorted = [...rows].sort((a, b) => b.updatedAt - a.updatedAt)
    if (!q) return sorted
    return sorted.filter((c) => c.title.toLowerCase().includes(q))
  }, [rows, query])

  // The chats that are blocked on a person, oldest wait first. This group
  // ignores the search field on purpose: a search narrows the lists below, and
  // a chat whose harness is starving should not be hidden by a query typed for
  // something else.
  const needs = useMemo(() => needsYou(rows), [rows])

  // The chat list is one Tab stop, not one per row. A working sidebar holds
  // dozens of chats, and a Tab per row (plus a Tab for each row's hover archive
  // button) put the first control of the view 40-odd Tabs away, which makes
  // the whole console unusable from the keyboard. Exactly one row carries
  // tabindex 0; the arrows move between rows, Home and End go to the ends, and
  // the archive button stays a pointer shortcut (Mod+Shift+A and the palette
  // already own archiving from the keyboard).
  const [rovingId, setRovingId] = useState(null)
  // What each waiting chat is holding, one line each, read once per question.
  // There is no clock here of its own: the sessions beat is the only thing
  // that changes `needs`, an answer is cached under the stamp it was read at,
  // and a chat that stops waiting is dropped, so a steady state reads
  // nothing. The line is a summary only - see the row below for the rule.
  const [questions, setQuestions] = useState({})
  const questionsRef = useRef(questions)
  questionsRef.current = questions
  const waitingKey = needs.chats.map((c) => questionKey(c.id, c.updatedAt)).join('|')
  useEffect(() => {
    if (!needs.chats.length) {
      if (Object.keys(questionsRef.current).length) setQuestions({})
      return undefined
    }
    const kept = pruneLines(questionsRef.current, wantedKeys(needs.chats))
    if (Object.keys(kept).length !== Object.keys(questionsRef.current).length) {
      questionsRef.current = kept
      setQuestions(kept)
    }
    const missing = missingLines(kept, needs.chats)
    if (!missing.length) return undefined
    let cancelled = false
    ;(async () => {
      // Every new question in one pass: the asks go out together and the lines
      // settle together, so three waiting chats cost one wait rather than
      // three, and the stamp that started the pass arrives complete instead of
      // re-entering between rows. Each ask owns its failure, so one chat whose
      // decisions cannot be read leaves its line empty and the others land.
      const asked = await Promise.all(
        missing.map(async (row) => {
          try {
            return { row, line: questionLine(await api.decisions(row.id)) }
          } catch {
            // A chat whose decisions cannot be read shows no line rather than
            // an error in the sidebar: the group already says it is waiting.
            return { row, line: null }
          }
        }),
      )
      if (cancelled) return
      let cache = { ...kept }
      for (const { row, line } of asked) {
        // Kept even when null, so a failed or empty read is not retried on
        // every beat; only a new question (a new stamp) asks again.
        cache = rememberLine(cache, questionKey(row.id, row.updatedAt), line)
      }
      setQuestions({ ...cache })
    })()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [waitingKey, api])

  // The two sections: registered projects with their chats, then the flat
  // Chats list - every chat that belongs to no project. A waiting chat is in
  // the group above instead, so no chat reads twice and each project's count
  // matches the rows under it.
  const rest = useMemo(() => {
    const waiting = new Set(needs.chats.map((c) => c.id))
    return visible.filter((c) => !waiting.has(c.id))
  }, [visible, needs])
  // Built once per projects change, not per row per beat: the grouping used
  // to scan every project's folders for every chat, which is chats x projects
  // x folders on every beat.
  const folders = useMemo(() => projectIndex(projects), [projects])
  const grouped = useMemo(() => sections(projects, rest, folders), [projects, rest, folders])
  // The names two or more projects share, so each head can say which folder it is.
  const sharedNames = useMemo(() => duplicatedNames(projects), [projects])

  // The ids that should carry the quiet mark. Ordering never consults it:
  // the sorts above keep the order they had, so nothing jumps under the
  // pointer when a mark appears or clears.
  const unread = useMemo(() => unreadIds(seen, rows), [seen, rows])
  // Which project groups hold an unread chat: one pass over the rows, where a
  // count per group re-walked every group.
  const unreadIn = useMemo(() => unreadProjects(rows, unread, folders), [rows, unread, folders])
  // How many of each project's chats are waiting. Those rows are drawn in the
  // group above, not under their project, so this is one pass over that same
  // group against the same folder index - the head's tally and the rows it
  // counts cannot disagree, and no chat is counted twice.
  const waitingIn = useMemo(() => waitingProjects(needs.chats, folders), [needs, folders])

  const toggleProject = (id) => {
    setCollapsed((c) => {
      const next = { ...c }
      if (next[id]) delete next[id]
      else next[id] = true
      storeCollapsed(next)
      return next
    })
  }

  // A search opens the projects it matches, whatever the person had folded
  // away: the grouped rows only draw while a project is open, so a folded
  // project would hide the one matching chat from the field the person is
  // typing in, and the search would look broken rather than narrow. The fold
  // comes back as soon as the query is cleared, because the stored map is
  // untouched by this.
  const openedBySearch = useMemo(() => {
    const ids = new Set()
    const q = query.trim().toLowerCase()
    if (!q) return ids
    for (const { project, items } of grouped.projects) {
      if (items.length > 0 || projectName(project).toLowerCase().includes(q)) ids.add(project.id)
    }
    return ids
  }, [grouped, query])

  // A project is folded only when the person folded it and no search is
  // showing something inside it.
  const folded = (id) => !!collapsed[id] && !openedBySearch.has(id)

  // The projects the daemon no longer lists are forgotten, so their ids cannot
  // fold a different project that takes the same id later.
  useEffect(() => {
    const known = new Set((projects ?? []).map((p) => p.id))
    setCollapsed((current) => {
      const ids = Object.keys(current)
      if (!ids.length) return current
      const kept = {}
      for (const id of ids) if (known.has(id)) kept[id] = true
      if (Object.keys(kept).length === ids.length) return current
      storeCollapsed(kept)
      return kept
    })
  }, [projects])

  // The keyboard's walk through the list goes in the order it reads: the
  // chats that need the user first (they are at the top of the list), then
  // each open project's chats, then the flat Chats list. A folded project's
  // chats are skipped, like the eye skips them - unless a search has opened it.
  // The walk visits what is drawn, not what is paged away: a row nobody can
  // see is a row the arrows must not land on.
  const searchRef = useRef(null)
  const planFor = (key, items, page) =>
    planList(items, {
      pins,
      page,
      expanded: showAll.has(key),
      keep: (row) => row.id === active,
    })
  const flatPlan = planFor('flat', grouped.flat, CHATS_PAGE)
  const order = [
    ...needs.chats,
    ...grouped.projects.flatMap(({ project, items }) => {
      if (folded(project.id)) return []
      const plan = planFor(project.id, items, PROJECT_PAGE)
      return [...plan.pinned, ...plan.shown]
    }),
    ...flatPlan.pinned,
    ...flatPlan.shown,
  ].map((row) => row.id)
  const step = (pick) => () => {
    if (!order.length) return false
    const at = order.indexOf(active)
    onSelect(pick(at))
  }
  useShortcut('next-chat', step((at) => order[at === -1 ? 0 : Math.min(at + 1, order.length - 1)]))
  useShortcut('prev-chat', step((at) => order[at === -1 ? order.length - 1 : Math.max(at - 1, 0)]))
  useShortcut('first-chat', step(() => order[0]))
  useShortcut('last-chat', step(() => order[order.length - 1]))

  // The row that holds the list's single Tab stop, and the arrows that move it.
  // `order` is the same sequence the next/prev-chat shortcuts walk and the same
  // order the rows are drawn in, so a row that is collapsed away cannot hold a
  // stop no one can see.
  const rovingRow = rovingId && order.includes(rovingId) ? rovingId : order[0] ?? null
  const moveRoving = (fromId, to) => {
    if (!order.length) return
    const at = order.indexOf(fromId)
    if (at < 0) return
    const next = to === 'first' ? 0 : to === 'last' ? order.length - 1 : at + to
    const id = order[Math.max(0, Math.min(order.length - 1, next))]
    if (!id || id === fromId) return
    setRovingId(id)
    document.querySelector(`[data-chat-row="${CSS.escape(id)}"]`)?.focus()
  }

  // Keep the keyboard's place when the row it was on disappears. Archiving the
  // roving chat from the palette, a search that no longer matches it, or
  // collapsing its project all remove the element that held the list's single
  // Tab stop. The stop itself is safe (`rovingRow` falls back to the first
  // row), but the browser drops focus to the body when the focused element
  // goes, so the next Tab starts at the top of the document instead of beside
  // where the person was. This puts the keyboard on the row that took its
  // place (same index, clamped) or the first row.
  //
  // It stays out of the way in both directions. It runs only when a row that
  // was drawn has gone AND focus is on the body, which is what a removed
  // element leaves behind; a focus that moved to another control clears the
  // memory instead, so a list nobody has touched never takes the keyboard.
  const orderKey = order.join('|')
  const drawnOrder = useRef([])
  const focusedRow = useRef(null)
  useEffect(() => {
    const before = drawnOrder.current
    drawnOrder.current = order
    const held = focusedRow.current
    if (!held || order.includes(held)) return
    if (document.activeElement && document.activeElement !== document.body) {
      focusedRow.current = null
      return
    }
    const at = before.indexOf(held)
    const target = order.length ? order[Math.min(Math.max(at, 0), order.length - 1)] : null
    focusedRow.current = target
    if (!target) return
    setRovingId(target)
    document.querySelector(`[data-chat-row="${CSS.escape(target)}"]`)?.focus()
    // `order` is read through the key: the array is rebuilt every render and
    // only its membership matters here.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [orderKey])
  useShortcut('search-chats', () => {
    if (!open) onReveal?.()
    // After the reveal renders, so the field is there to take focus.
    requestAnimationFrame(() => {
      searchRef.current?.focus()
      searchRef.current?.select()
    })
  })
  const hintNew = useHint('new-chat')
  const hintSearch = useHint('search-chats')

  // Attention without a toast: while the window cannot be looked at, the
  // document title carries how many chats are waiting. Looking at the window
  // again, or a waiting list that emptied, gives the console's own title back.
  // A background window still shows its title, so this is the quietest place
  // to put a count that is not a dialog. The desktop shell's real window
  // title and a taskbar flash are shell commands, which the console cannot
  // reach from here.
  const [watched, setWatched] = useState(true)
  const baseTitle = useRef('')
  // The string this effect last wrote. Anything else in the title at the next
  // beat means another writer owns it now, and that is the console's own title
  // from then on - so the base is read at the moment of the decision instead of
  // being a value cached at mount that can go stale.
  const writtenTitle = useRef(null)
  useEffect(() => {
    baseTitle.current = document.title
    const read = () => setWatched(!document.hidden && document.hasFocus())
    read()
    window.addEventListener('focus', read)
    window.addEventListener('blur', read)
    document.addEventListener('visibilitychange', read)
    return () => {
      window.removeEventListener('focus', read)
      window.removeEventListener('blur', read)
      document.removeEventListener('visibilitychange', read)
    }
  }, [])
  useEffect(() => {
    if (writtenTitle.current !== null && document.title !== writtenTitle.current) {
      baseTitle.current = document.title
    }
    const next = attentionTitle(baseTitle.current, needs.count, unread.size, watched)
    // Written only when it changed: a title that rewrote itself on every beat
    // would be worse than no title at all.
    if (document.title !== next) {
      writtenTitle.current = next
      document.title = next
    }
    return () => {
      // Leaving a title behind that somebody else wrote would be the same
      // rudeness as taking it in the first place, so the cleanup only puts the
      // console's own title back when the attention title is still ours.
      if (writtenTitle.current !== null && document.title !== writtenTitle.current) return
      const back = attentionTitle(baseTitle.current, 0, 0, true)
      if (document.title !== back) {
        writtenTitle.current = back
        document.title = back
      }
    }
  }, [needs.count, unread.size, watched])

  // The quiet mark's two halves. First sight of a chat is not news: a
  // backlog from before this existed, or a chat another device created,
  // gets its record at the stamp it had when this console first listed it.
  // Only what moves after that is marked.
  useEffect(() => {
    remember(seedSeen(seenRef.current, rows))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows])

  // Looking at a chat marks it read - the chat on screen, and only while the
  // window is actually being looked at (visible and focused), which is the
  // same fact the title effect above reads. A chat left open in a
  // background window has not been read.
  useEffect(() => {
    if (!watched || !active) return
    const row = rows.find((r) => r.id === active)
    if (!row) return
    remember(recordSeen(seenRef.current, row.id, row.updatedAt))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [watched, active, rows])

  const archive = (id) => {
    const row = rows.find((r) => r.id === id)
    setChatArchived(id, true)
    // Both ways in come through here (the hover button and the row's menu),
    // so the way back is offered whichever one the press came from.
    setUndo(row ? { id: row.id, title: row.title } : null)
    if (id === active) onSelect('new')
  }

  // The same call the Settings list makes. Restoring puts the chat back in
  // the list it was archived from, with its own record untouched.
  const undoArchive = () => {
    if (!undo) return
    setChatArchived(undo.id, false)
    setUndo(null)
  }

  // Copying a chat as Markdown from its own row. The rule and the two
  // sentences live in chat-markdown.js (the same module the palette's row
  // reads), so the two ways in cannot say different things; this only asks,
  // writes, and shows what came back. A refusal names itself rather than
  // leaving the row silently unchanged.
  const copyChat = (id) => {
    const chat = chats?.[id] ?? null
    const file = chat ? chatMarkdown(chat) : null
    if (!file) {
      setCopyNote('Nothing to copy in this chat.')
      return
    }
    const write = navigator.clipboard?.writeText?.(file.text)
    if (!write) {
      setCopyNote('Could not copy: this window has no clipboard.')
      return
    }
    write.then(() => setCopyNote(copiedLine(file))).catch((err) => setCopyNote(errorCopy(err)))
  }

  // The projects a chat can be moved to: out of every project first, then this
  // machine's projects. A chat's project is its folder, so the picker marks the
  // one the chat is in, and the daemon is the one that decides whether the
  // target is real.
  const moveTargets = () => [
    { id: 'none', name: 'No project' },
    ...(projects ?? []).map((project) => ({ id: project.id, name: projectName(project) })),
  ]

  // Where the chat is now, for the picker's mark: the project holding its
  // folder, or "none" when no project does.
  const currentProject = (row) =>
    (projects ?? []).find((project) => (project.folders ?? []).includes(row.workspace))?.id ?? 'none'

  // Moving a chat: the daemon decides and answers, and this says what it said.
  // A refusal keeps the daemon's own sentence, which names the reason (a turn
  // in flight, a project that is gone) better than anything this file could.
  const moveTo = async (id) => {
    const row = moving?.row
    setMoving(null)
    if (!row) return
    const project = id === 'none' ? null : id
    const request_id = `move-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`
    try {
      await api.call('POST', `/sessions/${row.id}/workspace`, {
        request_id,
        project_id: project,
      })
      const name = project ? projectName(projects.find((p) => p.id === project) ?? {}) : null
      setCopyNote(name ? `Moved to ${name}.` : 'Moved out of its project.')
    } catch (err) {
      setCopyNote(errorCopy(err))
    }
  }

  // Right-click on a chat: the same actions as the hover buttons, plus the
  // split directions - the same four words the pane header and the main
  // area's own menu use. `at` is where the menu opened: the move picker opens
  // there too, because the menu itself is gone by the time it is chosen.
  const menuItems = (row, at = null) => {
    const split = (dir, before) => () => onSplit(row.id, dir, before)
    const canSplit = onSplit && !isMobile
    // Read once, so the label and the icon cannot disagree about the row.
    const pinned = pins.includes(row.id)
    return [
      { label: 'Open', icon: <ChatCircle size={14} />, onSelect: () => onSelect(row.id) },
      // Pinning is an arrangement of the list, like Open is a way into the
      // chat: it sits at the top, before the edit and split rows, and it is
      // the one action whose label is the row's own state.
      {
        label: pinned ? 'Unpin' : 'Pin',
        icon: pinned ? <PushPinSlash size={14} /> : <PushPin size={14} />,
        onSelect: () => pinChat(row.id),
      },
      // Marking by hand, in the direction the row is drawn in: a chat with a
      // quiet mark offers to clear it, a read one offers to bring the mark
      // back (the person saw it on another device, or wants a reminder to
      // return). It sits beside Open because it is a look-level action on the
      // row, above the edit and split rows; the write goes through the same
      // remember path every other seen change uses.
      {
        label: unread.has(row.id) ? 'Mark as read' : 'Mark as unread',
        icon: unread.has(row.id) ? <CheckCircle size={14} /> : <Circle size={14} />,
        onSelect: () =>
          remember(markSeen(seenRef.current, row.id, row.updatedAt, unread.has(row.id) ? 'read' : 'unread')),
      },
      // Renaming is the first thing a new user needs and the last thing they
      // find: `/rename` exists, and the row's name is what the list's search
      // matches on. The dialog belongs to the shell (it is the same one the
      // palette's row opens), so this only asks for it.
      ...(onRenameChat
        ? [
            {
              label: 'Rename',
              icon: <PencilSimple size={14} />,
              onSelect: () => onRenameChat(row.id),
            },
          ]
        : []),
      // Moving a chat between projects is a fact the daemon keeps, not a view
      // state: the row asks, the picker names the target, and the route does it.
      // Only offered once the projects list has answered - before that the picker
      // could only say "No project" and a move would be a guess.
      ...(projectsAnswered
        ? [
            {
              label: 'Move to project',
              icon: <FolderSimple size={14} />,
              onSelect: () => setMoving({ row, at }),
            },
          ]
        : []),

      // Where the copy can work, and only there: a chat with nothing to write
      // here gets no row rather than one that answers "nothing to copy" after
      // a press. canCopyChat is the palette's rule as well.
      ...(canCopyChat(chats?.[row.id])
        ? [
            {
              label: 'Copy as Markdown',
              icon: <ClipboardText size={14} />,
              onSelect: () => copyChat(row.id),
            },
          ]
        : []),
      ...(canSplit
        ? [
            { divider: true },
            { label: 'Split right', icon: <ArrowRight size={14} />, onSelect: split('row', false) },
            { label: 'Split down', icon: <ArrowDown size={14} />, onSelect: split('col', false) },
            { label: 'Split left', icon: <ArrowLeft size={14} />, onSelect: split('row', true) },
            { label: 'Split up', icon: <ArrowUp size={14} />, onSelect: split('col', true) },
          ]
        : []),
      { divider: true },
      { label: 'Archive', icon: <Archive size={14} />, onSelect: () => archive(row.id) },
      // The daemon deletes synced chats only (it removes them from every
      // device); a private chat has no delete.
      ...(row.synced
        ? [
            {
              label: 'Delete chat',
              icon: <TrashSimple size={14} />,
              danger: true,
              onSelect: () => setDeleting(row),
            },
          ]
        : []),
      {
        label: 'Copy title',
        icon: <Copy size={14} />,
        // The same answer its neighbour gives: a refused clipboard used to be
        // swallowed here, so this row did nothing and said nothing while the row
        // above it in the same menu explained exactly this case.
        onSelect: () => {
          const write = navigator.clipboard?.writeText?.(row.title)
          if (!write) {
            setCopyNote('Could not copy: this window has no clipboard.')
            return
          }
          write.then(() => setCopyNote('Copied the title.')).catch((err) => setCopyNote(errorCopy(err)))
        },
      },
    ]
  }

  // The Mobile tile's phone fact: the phones of this Cloud and whether
  // one is reachable. The status is color, not words - a reachable phone
  // turns the tile green with a black glyph, away phones leave a hollow
  // corner dot. No phones, no mark at all.
  const phones = (devices ?? []).filter(isPhone)
  const phoneOnline = phones.some((d) => d.online)

  const chatRow = (row) => (
    <div
      key={row.id}
      className="chat-item"
      // A chat is a thing you can drag into a pane: the middle opens it
      // there, an edge band splits. Right-click and the palette still do
      // the same job for anyone who never drags.
      draggable
      onDragStart={(e) => {
        // Both types: the private one is what the panes look for (a text
        // selection carries text/plain too), the prefixed one is what a
        // reader from an older build understands.
        e.dataTransfer.setData(CHAT_DRAG_TYPE, row.id)
        e.dataTransfer.setData('text/plain', chatDragPayload(row.id))
        e.dataTransfer.effectAllowed = 'move'
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        // The rows are built here, once, and kept with the menu. A menu must
        // not change under the person: the copy row is gated on the chat's
        // transcript being in this window, and a transcript arriving while the
        // menu is open would otherwise insert a row and shift every later one
        // (the menu highlights by index), moving the target under the keyboard.
        setMenu({
          x: e.clientX,
          y: e.clientY,
          items: menuItems(row, { x: e.clientX, y: e.clientY }),
        })
      }}
    >
      <button
        className={`chat-row${row.id === active && !searchOn ? ' active' : ''}`}
        data-status={row.status}
        // The quiet mark: one white-alpha tint step (DESIGN.md's ladder),
        // not a colour and not a number. The status column keeps its own
        // language, so "not read" and "waiting" cannot be mistaken.
        data-unread={unread.has(row.id) ? 'true' : undefined}
        // The row's tooltip while its stream is between attempts: the same
        // word the pane header shows, so a transcript that stopped is not read
        // as an agent still thinking. No dot, no status change.
        title={reconnecting?.[row.id] ? `${row.title} - ${RECONNECTING}` : undefined}
        // The list's single Tab stop lives on one row (see rovingRow); the
        // arrows walk the whole list from wherever focus is, Home and End go
        // to the ends, and Enter is the button's own.
        data-chat-row={row.id}
        tabIndex={rovingRow === row.id ? 0 : -1}
        onFocus={() => {
          setRovingId(row.id)
          // Remember whose row held the keyboard: the effect above uses this to
          // tell "the row went under the keyboard" from "the person moved on".
          focusedRow.current = row.id
        }}
        onKeyDown={(e) => {
          const to = e.key === 'ArrowDown' ? 1 : e.key === 'ArrowUp' ? -1 : e.key === 'Home' ? 'first' : e.key === 'End' ? 'last' : null
          if (to == null) return
          e.preventDefault()
          moveRoving(row.id, to)
        }}
        onClick={() => onSelect(row.id)}
      >
        {/* the harness's mark leads - every harness, one
            sidebar, the at-a-glance signal of which agent owns
            each chat - sized to read from afar, then the title, with
            the status dot at the right edge so every status scans in
            one column */}
        {row.harness && <HarnessMark harness={row.harness} size={15} />}
        <div className="chat-titles">
          <span className="chat-title pv">{row.title}</span>
          {/* The visible mark a pinned row wears, in the title line so the
              list scans: a pin says the row sits at the top of this list by
              the person's own arrangement, not by its activity. */}
          {(row.pinned || pins.includes(row.id)) && (
            <span className="chat-pin" title="Pinned" aria-label="Pinned">
              <PushPin size={11} weight="fill" />
            </span>
          )}
          {/* What the harness is holding, one line, read only. The sidebar
              never answers, approves or dismisses anything: a decision is
              answered in the chat, where the harness's own words, its
              options and (for an approval) what it wants to run are on
              screen together. A summary here that could act would be an
              approvals panel built out of a row, without any of that. */}
          {row.question && <span className="chat-question pv">{row.question}</span>}
        </div>
        {mode === 'cloud' && row.device && (
          <span className="chat-device pv">{row.device}</span>
        )}
        {mode === 'local' && row.synced && (
          // What is true right now: the chat is set to sync. Its first copy
          // may still be on its way, so the mark promises the setting rather
          // than a delivery the next device has not confirmed.
          <Cloud size={11} className="chat-synced" aria-label="Syncs to your devices" />
        )}
      </button>
      <button
        type="button"
        className="chat-more"
        title="Chat actions"
        aria-label={`Actions for ${row.title}`}
        aria-haspopup="menu"
        tabIndex={-1}
        onClick={(e) => {
          e.stopPropagation()
          const rect = e.currentTarget.getBoundingClientRect()
          setMenu({ x: Math.round(rect.right), y: Math.round(rect.bottom + 6), items: menuItems(row) })
        }}
      >
        <DotsThree size={16} weight="bold" />
      </button>
    </div>
  )

  return (
    <aside className={`sidebar${open ? '' : ' collapsed'}`}>
      <div className="sb-actions">
        {/* Local is this computer's chats; Cloud is the synced chats of
            every device the user signed in on. A phone has only Cloud. */}
        {!isMobile && (
        <div className="mode-switch" data-mode={mode} role="group" aria-label="Connection mode">
          <span className="mode-switch-thumb" aria-hidden="true" />
          <button
            className={mode === 'local' ? 'on' : ''}
            aria-pressed={mode === 'local'}
            onClick={() => onMode('local')}
          >
            <House size={15} weight="duotone" />
            Local
          </button>
          <button
            className={mode === 'cloud' ? 'on' : ''}
            aria-pressed={mode === 'cloud'}
            onClick={() => onMode('cloud')}
          >
            <Cloud size={15} weight="duotone" />
            Cloud
          </button>
        </div>
        )}
        <button className="side-tile" onClick={() => onSelect('new')} title={hintNew('New chat')}>
          <Plus size={16} weight="bold" />
          New chat
        </button>
        <label className="side-tile search">
          <MagnifyingGlass size={16} weight="bold" className="search-icon" />
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onFocus={() => setSearchOn(true)}
            onBlur={() => setSearchOn(false)}
            onKeyDown={(e) => {
              // Esc in the search clears it, then leaves it.
              if (e.key === 'Escape') {
                e.preventDefault()
                if (query) setQuery('')
                else e.currentTarget.blur()
              }
            }}
            placeholder="Search"
            title={hintSearch('Search chats')}
            spellCheck={false}
          />
          {query && (
            <button className="search-clear" onClick={() => setQuery('')} title="Clear">
              <X size={12} weight="bold" />
            </button>
          )}
        </label>
        {/* Plugins and automations act on this computer, so the desktop
            shows them in Local and Cloud alike (a phone runs no chats and
            has neither); Machines belongs to Cloud and reveals and
            collapses with the mode switch (the grid-rows transition). */}
        <div className={`machines-reveal${isMobile ? ' closed' : ''}`}>
          <div className="machines-clip">
            <button
              className={`side-tile${active === 'plugins' && !searchOn ? ' on' : ''}`}
              onClick={() => onSelect('plugins')}
            >
              <PuzzlePiece size={18} weight="duotone" />
              Plugins
            </button>
            {/* The browser had one entry point, a globe among the floating icons, and a
                person looking for it found nothing. It is a tile now, beside the two views
                it works with, and the tile says whether the pane is open. */}
            {onBrowser && (
              <button
                className={`side-tile${browserOpen ? ' on' : ''}`}
                onClick={onBrowser}
                aria-pressed={Boolean(browserOpen)}
                title={
                  browserOpen
                    ? 'Close the browser pane'
                    : 'Open a browser beside the chat - its pages are the ones an agent can drive'
                }
              >
                <Globe size={17} weight="light" />
                Browser
              </button>
            )}
            <button
              className={`side-tile${active === 'automations' && !searchOn ? ' on' : ''}`}
              onClick={() => onSelect('automations')}
            >
              <Lightning size={18} weight="duotone" />
              Automations
            </button>
          </div>
        </div>
        <div className={`machines-reveal${mode === 'cloud' ? '' : ' closed'}`}>
          <div className="machines-clip">
            <button
              className={`side-tile${active === 'machines' && !searchOn ? ' on' : ''}`}
              onClick={() => onSelect('machines')}
            >
              <Desktop size={18} weight="duotone" />
              Machines
            </button>
          </div>
        </div>
      </div>

      {/* Pinned between the actions and the scrolled list, not inside it: the
          row that went is gone from whichever group held it, so there is no
          stable place in the list to put its way back, and a line that
          scrolls out of view is a line nobody reads. */}
      {undo && (
        <div className="chat-undo" role="status">
          <span className="chat-undo-text pv">Archived “{undo.title}”</span>
          <button
            type="button"
            className="chat-undo-act"
            aria-label={`Undo archiving ${undo.title}`}
            onClick={undoArchive}
          >
            Undo
          </button>
        </div>
      )}

      {/* The copy's answer, in the same slot and the same voice as the archive
          line above: one sentence, announced rather than shown as a toast. It
          is the sidebar's own line, so it stays where the row that asked is. */}
      {copyNote && (
        <div className="chat-undo" role="status">
          <span className="chat-undo-text pv">{copyNote}</span>
        </div>
      )}

      <nav className="sb-list" aria-label="Projects and chats">
        <div key={mode} className="sb-mode-fade">
        {/* The one group that exists only while it has something in it: a
            chat whose harness is blocked on an answer is starving, and it
            should not be hunted for in a long list. Same row, same mark, same
            amber dot - the status word is what puts it here. */}
        {needs.count > 0 && (
          <section className="sb-needs" aria-label="Needs you">
            <div className="section-title sb-needs-head">
              Needs you
              <span className="sb-needs-count">{needs.count}</span>
            </div>
            {needs.chats.map((row) => chatRow({ ...row, status: 'waiting', question: lineFor(questions, row.id, row.updatedAt) }))}
          </section>
        )}

        <div className="section-title sb-projects-head">
          Projects
          {/* The one way a folder becomes a project - the same flow the
              home screen's Add project button opens. */}
          <button
            type="button"
            className="sb-add-project"
            onClick={onAddProject}
            title="Add a project"
            aria-label="Add a project"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
              <path d="M6 1.25v9.5M1.25 6h9.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
            </svg>
          </button>
        </div>
        {grouped.projects.length === 0 && (
          <p className="sb-empty">
            {projectsAnswered ? 'No projects yet.' : 'Loading your projects…'}
          </p>
        )}
        {grouped.projects.map(({ project, items }) => {
          const isCollapsed = folded(project.id)
          // This project's rows as the list draws them: its pinned chats in pin
          // order, then the first page of the rest. The plan is built from the
          // group's own rows, so a chat moved to another project pins there.
          const plan = planFor(project.id, items, PROJECT_PAGE)
          // One reader of a project's name: projects.js owns the rule (and
          // answers "project" for a project whose folders are gone, where the
          // inline split this replaced would have thrown). A head whose name another
          // head also carries says which folder it is: two folders that end in the same
          // basename were otherwise identical rows, Remove and New chat included.
          const name = projectName(project)
          // A project whose folder is gone is not a live project called "project": its
          // chats have already fallen to the flat list, and there is nothing on disk to
          // point at, so the head says what it is.
          const folderIsGone = folderless(project)
          const shownName = folderIsGone ? 'A project with no folder' : name
          const duplicated = sharedNames.has(name)
          const nameSuffix = duplicated ? project.folders?.[0] : null
          const isArmed = rowIsArmed(armed, project.id)
          // What the head says beside the name, or null when it has nothing to
          // add: the count answers a collapsed head, the waiting count is the
          // one fact the expanded list does not show at a glance. Both numbers
          // were counted in the grouping pass (projects.js), so this is only
          // formatting.
          const held = projectLine({ chats: items.length, waiting: waitingIn.get(project.id) ?? 0 }, { collapsed: isCollapsed })
          return (
            <section
              key={project.id}
              className={`project${isArmed ? ' armed' : ''}`}
              // Focus or a click leaving the row is a decision to keep it:
              // the armed state never survives a look away.
              onBlur={(e) => {
                if (isArmed && !e.currentTarget.contains(e.relatedTarget)) {
                  setArmed(clearRow())
                }
              }}
            >
              <button
                className="project-head"
                // Something inside is unread: the mark is the quiet step, and it
                // never carries a count of its own. The line beside the name is
                // the head's own sentence about what the project holds, below.
                data-unread={unreadIn.has(project.id) ? 'true' : undefined}
                onClick={() => toggleProject(project.id)}
                aria-expanded={!isCollapsed}
              >
                <span className={`project-caret${isCollapsed ? ' closed' : ''}`}>
                  <CaretDown size={12} />
                </span>
                <span className="project-name pv">{shownName}</span>
                {nameSuffix && (
                  <span className="project-count-part project-folder pv" title={nameSuffix}>
                    {nameSuffix}
                  </span>
                )}
                {held && (
                  <span className="project-count">
                    {held.map((part) => (
                      <span
                        key={part.kind}
                        className={`project-count-part${part.kind === 'waiting' ? ' is-waiting' : ''}`}
                      >
                        {part.text}
                      </span>
                    ))}
                  </span>
                )}
              </button>
              {isArmed ? (
                <>
                  {/* The trash is replaced in place by the word that acts, so
                      a second press is a press on the same spot and cannot
                      land on the `+` two pixels away. It is always visible
                      while armed (a hover-only confirm is easy to miss) and
                      Escape keeps. The line under it says what the press
                      does; one fragment holds the pair, because a ternary
                      branch is one expression. */}
                  <button
                    type="button"
                    className="sb-unregister"
                    autoFocus
                    title="Unregister this project. Its chats move to Chats and the folder on disk is never touched."
                    aria-label={`Unregister ${shownName}`}
                    aria-describedby="sb-unregister-note"
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') {
                        e.preventDefault()
                        setArmed(clearRow())
                      }
                    }}
                    onClick={() => {
                      setArmed(clearRow())
                      removeProject(project.id)
                    }}
                  >
                    Unregister
                  </button>
                  <p className="sb-unregister-note" id="sb-unregister-note">
                    The project goes; its chats move to Chats and the folder on disk is never touched.
                  </p>
                </>
              ) : (
                <>
                  {/* A new chat already in this project, straight from its
                      row: no trip through the home screen's project picker. */}
                  <button
                    type="button"
                    className="sb-project-new"
                    title={`New chat in ${shownName}`}
                    aria-label={`New chat in ${shownName}`}
                    onClick={() => onNewChatIn?.(project.id)}
                  >
                    <Plus size={12} weight="bold" />
                  </button>
                  {/* Unregistering keeps the chats - they fall to the flat
                      list below; the folder on disk is never touched. It asks
                      once first, because that click sits beside the `+`. */}
                  <button
                    type="button"
                    className="sb-project-remove"
                    title="Remove the project - it asks once, and its chats stay"
                    aria-label={`Remove ${shownName}`}
                    onClick={() => setArmed((current) => armRow(current, project.id))}
                  >
                    <TrashSimple size={11} />
                  </button>
                </>
              )}
              {!isCollapsed && items.length === 0 && (
                <p className="sb-empty sb-project-empty">
                  {folderIsGone
                    ? 'No folder behind this project any more. Unregister it to clear the row.'
                    : sessionsAnswered
                      ? 'No chats yet.'
                      : 'Loading your chats…'}
                </p>
              )}
              {!isCollapsed && plan.pinned.map((row) => chatRow({ ...row, pinned: true }))}
              {!isCollapsed && plan.shown.map(chatRow)}
              {!isCollapsed && plan.next > 0 && (
                // A page control, not a chat row: the list's one Tab stop stays
                // on the rows, and this line is reachable like the head's own
                // buttons. It reveals the rest of this list for this session.
                <button type="button" className="sb-more" onClick={() => showMore(project.id)}>
                  Show {plan.next} more
                </button>
              )}
            </section>
          )
        })}

        <div className="section-title sb-chats-title" data-unread={listHasUnread(grouped.flat, unread) ? 'true' : undefined}>
          Chats
        </div>
        {/* Only while there are chats to be outside a project: with every
            project at zero, the projects say it and this line would be a
            second sentence about nothing. */}
        {grouped.projects.length > 0 && grouped.flat.length === 0 && rows.length > 0 && (
          <p className="sb-empty">
            {sessionsAnswered ? 'No chats outside projects.' : 'Loading your chats…'}
          </p>
        )}
        {flatPlan.pinned.map((row) => chatRow({ ...row, pinned: true }))}
        {flatPlan.shown.map(chatRow)}
        {flatPlan.next > 0 && (
          <button type="button" className="sb-more" onClick={() => showMore('flat')}>
            Show {flatPlan.next} more
          </button>
        )}

        {/* With a project registered and nothing running, the project's own row
            says it has no chats yet; the generic line would say it twice. An
            unanswered first poll is not "no chats yet": until the list has
            answered, the line says it is still reading. */}
        {rows.length === 0 && grouped.projects.length === 0 && (
          <p className="sb-empty">
            {!sessionsAnswered
              ? 'Loading your chats…'
              : query.trim()
                ? `No chats match “${query.trim()}”.`
                : mode === 'cloud'
                  ? 'No synced chats yet.'
                  : 'No chats yet.'}
          </p>
        )}
        {rows.length > 0 && query.trim() && visible.length === 0 && (
          <p className="sb-empty">No chats match “{query.trim()}”.</p>
        )}
        </div>
      </nav>

      <footer className="sb-foot">
        {/* The footer's views, as icons: account, mobile, metrics, then
            settings. Each opens its view in the primary pane. The browser
            lives in the homepage's floating bar instead. */}
        <div className="sb-foot-row" role="group" aria-label="Account, mobile, metrics, settings">
          <button
            type="button"
            className={`sb-icon-btn${active === 'account' && !searchOn ? ' on' : ''}`}
            aria-pressed={active === 'account'}
            title="Account"
            aria-label="Account"
            onClick={() => onSelect('account')}
          >
            <UserCircle size={22} weight="duotone" />
          </button>
          <button
            type="button"
            className={`sb-icon-btn${phones.length ? (phoneOnline ? ' sb-phone--online' : ' sb-phone--offline') : ''}${active === 'mobile' && !searchOn ? ' on' : ''}`}
            aria-pressed={active === 'mobile'}
            title={
              !phones.length
                ? 'Mobile'
                : phoneOnline
                  ? `Phone online${phones.length > 1 ? ` (${phones.filter((d) => d.online).length} of ${phones.length})` : ''}`
                  : 'Phone away'
            }
            aria-label="Mobile"
            onClick={() => onSelect('mobile')}
          >
            <DeviceMobile size={20} weight="duotone" />
            {/* The status is color, not words: a reachable phone tints
                the tile green with a black glyph; an away one leaves a
                hollow dot in the tile's corner. */}
            {phones.length > 0 && !phoneOnline && (
              <span className="sb-phone-dot" aria-hidden="true" />
            )}
          </button>
          <button
            type="button"
            className={`sb-icon-btn${active === 'metrics' && !searchOn ? ' on' : ''}`}
            aria-pressed={active === 'metrics'}
            title="Metrics"
            aria-label="Metrics"
            onClick={() => onSelect('metrics')}
          >
            <ChartBar size={20} weight="duotone" />
          </button>
          <button
            type="button"
            className={`sb-icon-btn${active === 'settings' && !searchOn ? ' on' : ''}`}
            aria-pressed={active === 'settings'}
            title="Settings"
            aria-label="Settings"
            onClick={() => onSelect('settings')}
          >
            <Gear size={20} weight="duotone" />
          </button>
        </div>
      </footer>
      {menu && (
        // The menu's own rows, held from the moment it opened: see the
        // onContextMenu above for why they are not rebuilt here.
        <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />
      )}
      {moving && (
        // The project picker the app already has, opened where the row's menu
        // was: "No project" first, then this machine's projects, the chat's
        // own marked. The daemon decides, and its words are what a failure
        // shows.
        <PickerMenu
          label="Move to project"
          items={moveTargets()}
          selectedId={currentProject(moving.row)}
          anchor={moving.at ?? undefined}
          onChoose={moveTo}
          onClose={() => setMoving(null)}
        />
      )}
      {deleting && (
        <DeleteChatModal
          title={deleting.title}
          onClose={() => setDeleting(null)}
          onDelete={async () => {
            await deleteChat(deleting.id)
            if (deleting.id === active) onSelect('new')
          }}
        />
      )}
    </aside>
  )
}
