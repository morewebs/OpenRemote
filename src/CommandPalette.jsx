// The command palette (Ctrl+K): one search over everything you can do or
// open - actions, chats, views, projects, machines - grouped, best match
// first, each row with its key on the right. An empty query shows recent
// chats and a few suggestions. "Switch model…", "Set effort…" and "Set
// permission…" open their own list inside the palette; Esc or Backspace
// on an empty search steps back out.

import { useEffect, useMemo, useRef, useState } from 'react'
import {
  ArrowRight,
  BookmarkSimple,
  ChartBar,
  Check,
  ClipboardText,
  ClockCounterClockwise,
  Command,
  Desktop,
  DeviceMobile,
  FileCode,
  FloppyDisk,
  FolderOpen,
  Gear,
  Globe,
  HardDrives,
  Keyboard,
  Lightning,
  MagnifyingGlass,
  PencilSimple,
  Play,
  Plus,
  PuzzlePiece,
  Repeat,
  SquaresFour,
  StopCircle,
  TrashSimple,
  UserCircle,
} from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { useDialog } from './dialog-focus.js'
import { useKeyboard, isMac } from './keyboard.jsx'
import { ACTIONS, comboFromEvent, comboLabel, rankMatches, tieRule } from './shortcuts.js'
import { reopenAvailable, reopenClosed, browserMounted, getState, setState, subscribe } from './browser-store.js'
import { BROWSER_GROUP, browserTabs } from './palette-tabs.js'
import { chatTitle } from './shape.js'
import { canSeedChat, chatSetup } from './chat-setup.js'
import { pickable } from './harness-manager.js'
import { activeSessions } from './archive.js'
import { hasFileChanges } from './changes.js'
import { chatMarkdown, copiedLine } from './chat-markdown.js'
import { errorCopy } from './error-copy.js'
import { planJob, requestJob } from './chat-to-job.js'
import { openChanges } from './ChangesModal.jsx'
import PresetNameModal from './PresetNameModal.jsx'
import { presetFromChat, savePreset } from './presets.js'
import {
  elsewhereAside,
  namedChats,
  runningLabel,
  stopQuestion,
  stopSummary,
  stopTargets,
  stoppingLine,
} from './stop-everything.js'
import {
  HARNESS_EMPTY_LINE,
  HARNESS_PICK_LINE,
  HARNESS_STOP_NAME,
  harnessQuestion,
  harnessStopSub,
  harnessStopping,
  harnessStops,
  harnessSummary,
} from './stop-harness.js'
import { sessionGone } from './api/daemon.js'
import { harnessName } from './harness-names.js'
import { HarnessMark } from './brand-marks.jsx'
import { isPhone } from './cloud.js'
import { chatStatus } from './state/reducer.js'
import { loadSeen, recordSeen, storeSeen } from './unread.js'
import { markAllSeen, movedLine, sinceLastLook, whatHappened, whenLabel } from './since-last-look.js'
import { SAVED_LAYOUTS_NONE, capacityLine, loadLayouts, restoreShape, saveLayouts, withoutLayout } from './layouts.js'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { projectName } from './projects.js'
import {
  RULES_SECTION,
  ruleAnswer,
  ruleRunAvailable,
  ruleRunRows,
  runRule,
  subscribeRuleRun,
} from './rule-run.js'
import './keyboard.css'

const VIEW_ICONS = {
  'view-settings': <Gear size={16} weight="duotone" />,
  'view-automations': <Lightning size={16} weight="duotone" />,
  'view-plugins': <PuzzlePiece size={16} weight="duotone" />,
  'view-machines': <HardDrives size={16} weight="duotone" />,
  'view-account': <UserCircle size={16} weight="duotone" />,
  'view-mobile': <DeviceMobile size={16} weight="duotone" />,
  'view-metrics': <ChartBar size={16} weight="duotone" />,
}

// The lists an action opens inside the palette, and what each is called.
const PAGES = [
  { kind: 'model', name: 'Switch model…' },
  { kind: 'effort', name: 'Set effort…' },
  { kind: 'permission', name: 'Set permission…' },
]

// Rules sit right under Actions: a person who opens the palette to run one has
// ten of them, and the alternative is scrolling past every chat first.
const SECTION_ORDER = ['Actions', 'Rules', 'Chats', 'Views', 'Projects', 'Machines', 'Browser']
const SECTION_CAP = 8

// The pages with no keyboard source behind them, so the crumb has a name.
const PAGE_TITLES = { since: 'Since your last look', 'caught-up': 'Since your last look', layouts: 'Layouts', 'rule-run': 'Rules' }

export default function CommandPalette({ recentChatIds, onClose, onOpenChat, onOpenProject, onOpenMachine, onRace, raceFree, panes = 1, paneCapacity = 4, onSaveLayout, onRestoreLayout, onRenameChat = null, onNewWithSetup = null, focusedChatId = null }) {
  // One line on purpose: the stop-everything wiring test reads this call to
  // prove the palette stops chats through the action a single chat already
  // uses. The two new names sit before `stopChat`, which stays last.
  const { sessions, chats, archived, projects, devices, thisDevice, capabilities, machines, automations, machinesAnswered, stopChat } = useConsole()
  const kb = useKeyboard()
  const [query, setQuery] = useState('')
  const [page, setPage] = useState(null)
  const [cursor, setCursor] = useState(0)
  // The one line after a copy: kept inside the palette rather than closing to
  // say it somewhere else.
  const [copied, setCopied] = useState(null)
  // Stop everything: the plan the question was built from (so the numbers
  // in it cannot move under the user between the ask and the press), the
  // phase, and the sentence that settles it.
  const [stopPlan, setStopPlan] = useState(null)
  const [stopPhase, setStopPhase] = useState(null)
  const [stopResult, setStopResult] = useState(null)
  // One harness's chats: the harness being confirmed, the plan it was built
  // from (pinned for the same reason), the phase, and the sentence that
  // settles it.
  const [harnessStopId, setHarnessStopId] = useState(null)
  const [harnessPlan, setHarnessPlan] = useState(null)
  const [harnessPhase, setHarnessPhase] = useState(null)
  const [harnessResult, setHarnessResult] = useState(null)
  // The preset being named. It lives here, not in the shell, because the modal
  // is drawn over this list and the list stays open underneath it (see the
  // savePreset arm in choose).
  const [namingPreset, setNamingPreset] = useState(null)
  const listRef = useRef(null)
  const inputRef = useRef(null)
  const panelRef = useRef(null)
  // The palette is a dialog like the others: focus in on open, Tab stays
  // inside, the app behind goes inert, and the opener gets the focus back when
  // it closes. `esc: false` on purpose - Escape here also steps back out of a
  // page, so the palette keeps its own handler for that and the shared hook
  // must not consume the key first.
  useDialog(panelRef, onClose, { esc: false })

  // What moved while nobody was reading: unread.js owns the record of what has
  // been seen, so this reads it once per palette and hands the same map to the
  // page that marks a chat read.
  const [seen, setSeen] = useState(() => loadSeen())
  // The poll, in the shape the unread rules read: the sidebar's own status
  // words and the activity stamp, one row per chat that is not archived.
  const moveRows = useMemo(
    () =>
      activeSessions(sessions ?? [], archived).map((session) => ({
        id: session.id,
        title: chatTitle(session),
        harness: session.harness,
        updatedAt: session.updated_at ?? 0,
        status: chatStatus(session.status),
      })),
    [sessions, archived],
  )
  const moved = useMemo(() => sinceLastLook(moveRows, seen), [moveRows, seen])

  // The saved layouts, read once per palette like the seen map: this console's
  // own shapes, on this computer. Which chat is current decides what a chat
  // pane opens on restore, and the address is the only honest answer the
  // palette has to "this chat" (the same reading the review row above uses).
  const [savedLayouts, setSavedLayouts] = useState(() => loadLayouts())
  const [armedLayout, setArmedLayout] = useState(null)
  const [layoutNote, setLayoutNote] = useState(null)
  // The Rules group exists only while the Automations view is mounted to run
  // one: the action is a register/request pair (rule-run.js), and a group of
  // rows whose press does nothing is worse than a group that is not there.
  const [rulesCanRun, setRulesCanRun] = useState(() => ruleRunAvailable())
  useEffect(() => subscribeRuleRun(() => setRulesCanRun(ruleRunAvailable())), [])
  // What the Rules group answered: one line under the search field, in the
  // same shape every command that asked nothing uses.
  const [ruleRunState, setRuleRunState] = useState(null)
  // The browser's tabs, live. The Browser group lists pages, and a page that
  // closed (or a title that arrived) while the palette is open would otherwise
  // leave a row pointing at a tab that is gone: the store is the one list of
  // them, so the group follows it the same way the strip does.
  const [tabs, setTabs] = useState(() => getState().tabs)
  useEffect(() => subscribe(() => setTabs(getState().tabs)), [])
  // The Browser rows, in their own memo, the way chatActions below builds its
  // two rows apart: a page reports its title, its url, its favicon and its
  // loading state one at a time, and every one of those changes the tab list.
  // The group has to follow them; the action, chat, project and rule rows do
  // not, so they are not rebuilt for it. The key is the tab list plus the
  // mount - a mounted browser with no tabs and an unmounted one both mean no
  // group, but they are different facts and the mount can change under an open
  // palette. Until either changes the rows keep their identity.
  const browserIsMounted = browserMounted()
  const tabRows = useMemo(
    () =>
      browserTabs({ tabs, mounted: browserIsMounted }).map((tab) => ({
        key: `tab:${tab.key}`,
        section: BROWSER_GROUP,
        name: tab.name,
        sub: tab.sub,
        icon: <Globe size={15} />,
        run: { tab: tab.key },
      })),
    [tabs, browserIsMounted],
  )
  const onScreen = typeof location === 'undefined' ? '' : location.hash.replace(/^#/, '')
  const currentChat = (sessions ?? []).some((session) => session.id === onScreen) ? onScreen : null

  // Everything the palette can offer right now, as rows.
  // The only two rows that read a chat's timeline. Their content changes only
  // when their own answer does - which chat is on screen, whether it has file
  // changes, whether it has items - so a token growing a timeline re-checks
  // two booleans here instead of rebuilding every row in the list.
  const chatActions = useMemo(() => {
    const onScreen = typeof location === 'undefined' ? '' : location.hash.replace(/^#/, '')
    const reviewChat = chats?.[onScreen] ?? null
    const markdownChat = focusedChatId ? chats?.[focusedChatId] ?? null : null
    return {
      onScreen,
      reviewChat,
      markdownChat,
      key: [
        onScreen,
        focusedChatId ?? '',
        reviewChat && hasFileChanges(reviewChat.timeline) ? 'changes' : '',
        markdownChat && (markdownChat.timeline ?? []).length > 0 ? 'items' : '',
        reviewChat?.id ?? '',
        markdownChat?.id ?? '',
      ].join('|'),
    }
  }, [chats, focusedChatId])
  // Row identity across beats: the memo above runs on every token, the rows it
  // produces are reused while its key is unchanged. The address is already read
  // once above, so only the two rows this memo decided are taken from here.
  const chatActionsRef = useRef({ key: null, onScreen: '', reviewChat: null, markdownChat: null })
  if (chatActionsRef.current.key !== chatActions.key) chatActionsRef.current = chatActions
  const { reviewChat, markdownChat } = chatActionsRef.current

  // The setup the focused chat implies, read by the same rule the shell applies
  // when the row is pressed (chat-setup.js). Read here only to decide whether
  // the row can deliver anything: a chat whose harness this console cannot
  // offer would open a New chat on another harness and call it the setup the
  // person asked to copy.
  //
  // The record is read on every render (its model can change mid-stream), but
  // the setup is rebuilt only when one of its own fields does: its key is those
  // fields, so a token that merely grows the timeline keeps the row's identity,
  // the same reason chatActions above carries a key.
  const setupRecord = focusedChatId
    ? chats?.[focusedChatId] ?? (sessions ?? []).find((s) => s.id === focusedChatId) ?? null
    : null
  const setupFields = setupRecord
    ? [
        focusedChatId,
        setupRecord.harness,
        setupRecord.model,
        setupRecord.effort,
        setupRecord.permissionMode ?? setupRecord.permission_mode,
        setupRecord.workspace,
        setupRecord.executor,
      ].join('|')
    : ''
  const rowSetup = useMemo(
    () =>
      setupRecord
        ? chatSetup(setupRecord, {
            harnesses: (capabilities?.harnesses ?? []).filter(pickable),
            projects: projects ?? [],
            thisDevice,
          })
        : null,
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [setupFields, capabilities, projects, thisDevice],
  )

  const everything = useMemo(() => {
    const keys = (id) => comboLabel(kb.bindings[id], isMac)
    const actions = ACTIONS.filter((a) => a.id !== 'palette' && kb.available(a.id)).map((a) => ({
      key: `action:${a.id}`,
      section: a.id.startsWith('view-') ? 'Views' : 'Actions',
      name: a.label,
      icon: VIEW_ICONS[a.id] ?? (a.id === 'cheat-sheet' ? <Keyboard size={15} /> : <Command size={15} />),
      keys: keys(a.id),
      run: { action: a.id },
    }))
    const pages = PAGES.filter((p) => (kb.source(p.kind)?.items ?? []).length > 0).map((p) => ({
      key: `page:${p.kind}`,
      section: 'Actions',
      name: p.name,
      icon: <ArrowRight size={15} />,
      run: { page: p.kind },
    }))
    // The chat on screen: the shell writes the address on every navigation,
    // and the palette gets no prop about the view, so the address is the
    // only honest answer to "this chat". A review of a chat the user cannot
    // see would be a lie, so the row appears only when the address names a
    // chat this window holds and that chat has file changes to read.
    const review = reviewChat && hasFileChanges(reviewChat.timeline)
      ? [
          {
            key: 'action:changes',
            section: 'Actions',
            name: 'Changes in this chat',
            icon: <FileCode size={15} />,
            run: { changes: reviewChat.id },
          },
        ]
      : []
    // Copying this chat. Deliberately not `reviewChat` above: that one reads
    // the address, which names the primary pane's view, and a split can have
    // its focus on a different chat - the clipboard would then hold another
    // chat's transcript while the person is looking at this one, and they
    // would paste it somewhere believing it was the chat on screen. The shell
    // owns the focused pane and passes its chat in, the same seam the rename
    // row uses.
    const markdown = markdownChat && (markdownChat.timeline ?? []).length > 0
      ? [
          {
            key: 'action:markdown',
            section: 'Actions',
            name: 'Copy this chat as Markdown',
            icon: <ClipboardText size={15} />,
            run: { markdown: markdownChat.id },
          },
        ]
      : []
    const chatRows = activeSessions(sessions ?? [], archived).map((s) => ({
      key: `chat:${s.id}`,
      section: 'Chats',
      name: chatTitle(s),
      sub: harnessName(s.harness),
      icon: <HarnessMark harness={s.harness} size={15} />,
      veiled: true,
      run: { chat: s.id },
    }))
    const projectRows = (projects ?? []).map((p) => ({
      key: `project:${p.id}`,
      section: 'Projects',
      name: projectName(p),
      sub: p.folders[0],
      subVeiled: true,
      icon: <FolderOpen size={15} />,
      veiled: true,
      run: { project: p.id },
    }))
    // The Machines group's rows, built from the account's devices. Named apart
    // from the console's own `machines` on purpose: that list is what the rule
    // predicate reads (rule-run.js looks for `{ machine: { id } }` rows), and a
    // local `machines` shadowing it made every rule look like it named a machine
    // the account no longer has, so the Rules group could never appear and
    // "Plan this chat as a rule" was refused with "machine" in every profile.
    const machineRows = (devices ?? [])
      .filter((d) => !isPhone(d))
      .map((d) => ({
        key: `machine:${d.id}`,
        section: 'Machines',
        name: d.name,
        sub: d.id === thisDevice ? 'This computer' : d.online ? 'Online' : 'Offline',
        icon: d.kind === 'machine' ? <HardDrives size={15} /> : <Desktop size={15} />,
        veiled: true,
        run: { machine: d.id },
      }))
    // The one command that ends every running chat. Offered only while
    // something is running: a command that would end nothing is noise in
    // a list that is searched by name.
    const targets = stopTargets(sessions ?? [], { thisDevice })
    const stopRow = targets.running.length
      ? [
          {
            key: 'action:stop-all',
            section: 'Actions',
            name: 'Stop all running chats',
            sub: runningLabel(targets.running.length),
            icon: <StopCircle size={15} />,
            run: { stopAll: true },
          },
        ]
      : []
    // The same command narrowed to one harness. Only harnesses this computer
    // can actually stop are listed, and the page is not offered when that
    // list is empty: a page of nothing is noise next to the command that
    // would have said the same thing in one row.
    const harnessRows = harnessStops(sessions ?? [], { thisDevice })
    const harnessRow = harnessRows.length
      ? [
          {
            key: 'action:stop-harness',
            section: 'Actions',
            name: HARNESS_STOP_NAME,
            sub: harnessStopSub(harnessRows),
            icon: <StopCircle size={15} />,
            run: { stopHarnessPage: true },
          },
        ]
      : []
    // The chat on screen as a rule. chat-to-job.js decides whether the chat
    // can seed one at all: with no first task, no harness a rule could name,
    // no machine online or no workspace to carry, the row is not offered
    // rather than opening the editor on a hole.
    const job = planJob(reviewChat, {
      harnesses: capabilities?.harnesses ?? [],
      machines: machines ?? [],
      projects: projects ?? [],
    })
    const jobRow = job.offer
      ? [
          {
            key: 'action:chat-job',
            section: 'Actions',
            name: 'Turn this chat into a recurring job',
            icon: <Repeat size={15} />,
            run: { job: job.draft },
          },
        ]
      : []
    // One prompt into several harnesses at once: the console's own thesis as an
    // action. A race opens a pane per harness, so a window with fewer than two
    // free says why in the row's own words instead of offering a start the
    // modal would have to refuse.
    const raceShort = (raceFree ?? 0) < 2
    const raceRow = [
      {
        key: 'action:race',
        section: 'Actions',
        name: 'Race a prompt across harnesses',
        ...(raceShort ? { sub: 'No free pane' } : {}),
        icon: <Repeat size={15} />,
        run: { race: true },
      },
    ]
    // The page of what moved, offered only while something did: a command that
    // opens an empty page costs a keystroke to learn nothing.
    const sinceRow = moved.length
      ? [
          {
            key: 'action:since',
            section: 'Actions',
            name: 'Since your last look',
            sub: movedLine(moved.length),
            icon: <ClockCounterClockwise size={15} />,
            run: { since: true },
          },
          // Marking everything read acts on the same list the page above shows,
          // so the two can never disagree about what counts as moved. It asks
          // nothing and answers with one sentence afterwards.
          {
            key: 'action:mark-read',
            section: 'Actions',
            name: 'Mark everything as read',
            sub: `${moved.length} ${moved.length === 1 ? 'chat' : 'chats'}`,
            icon: <Check size={15} />,
            run: { markAll: true },
          },
        ]
      : []
    // Saving a shape needs a shape: one pane is just the window. Restoring one
    // is offered while this console holds any, which is the only time the page
    // could list anything.
    const layoutRow = panes > 1
      ? [
          {
            key: 'action:save-layout',
            section: 'Actions',
            name: 'Save this layout…',
            icon: <FloppyDisk size={15} />,
            run: { saveLayout: true },
          },
        ]
      : []
    const layoutsRow = savedLayouts.length
      ? [
          {
            key: 'action:layouts',
            section: 'Actions',
            name: 'Layouts',
            sub: `${savedLayouts.length} saved`,
            icon: <SquaresFour size={15} />,
            run: { layouts: true },
          },
        ]
      : []
    // Renaming the chat the focused pane is showing. Deliberately not
    // `reviewChat` above: that one reads the address, which names the primary
    // pane's view, and a split can have its focus on a different chat - a row
    // built from the address would rename one chat while the user is looking
    // at another, which is the one way a rename can lie. The shell owns the
    // focused pane and passes its chat in; the shell hosts the dialog too, so
    // the row only asks for it.
    const renameRow =
      focusedChatId && onRenameChat
        ? [
            {
              key: 'action:rename-chat',
              section: 'Actions',
              name: 'Rename chat',
              sub: chats?.[focusedChatId]?.title ?? 'This chat',
              icon: <PencilSimple size={15} />,
              run: { renameChat: focusedChatId },
            },
          ]
        : []
    // "New chat with this setup": the same chat the rename row means, opened
    // again with its harness, model, effort, approval word and folder. Offered
    // only when the rule can deliver a setup, so the row never promises a New
    // chat on a harness this console cannot start.
    const setupRow =
      focusedChatId && onNewWithSetup && canSeedChat(rowSetup)
        ? [
            {
              key: 'action:new-with-setup',
              section: 'Actions',
              name: 'New chat with this setup',
              sub: chats?.[focusedChatId]?.title ?? 'This chat',
              subVeiled: true,
              icon: <Plus size={15} />,
              run: { newWithSetup: focusedChatId },
            },
          ]
        : []
    // Saving this chat's setup as a New chat preset. The plan carries what the
    // preset would hold and refuses with a reason when the chat cannot become
    // one, so the row appears only where pressing it would mean something.
    const presetPlan = presetFromChat(reviewChat, {
      harnesses: capabilities?.harnesses ?? [],
      projects: projects ?? [],
      thisDevice,
    })
    const presetRow = presetPlan.offer
      ? [
          {
            key: 'action:save-preset',
            section: 'Actions',
            name: 'Save this setup as a preset',
            sub: presetPlan.suggestedName,
            icon: <BookmarkSimple size={15} />,
            run: { savePreset: presetPlan.preset },
          },
        ]
      : []
    // The browser's one-level-back: the last tab closed in the browser pane.
    // Only while a mounted browser can actually do it - the store registers
    // that action from the view, so the row is never a promise with no page
    // behind it.
    const reopenRow = reopenAvailable()
      ? [
          {
            key: 'action:reopen-closed-tab',
            section: 'Actions',
            name: 'Reopen closed tab',
            sub: 'The last tab closed in the browser',
            keys: 'Ctrl+Shift+T',
            icon: <Repeat size={15} />,
            run: { reopenClosed: true },
          },
        ]
      : []
    // The view's own list, filtered to what can actually run: a paused rule,
    // or one whose machine is gone or has not come online, is not offered
    // (rule-run.js owns that rule and its copy).
    const ruleRows = rulesCanRun
      ? ruleRunRows(automations, machines, { answered: machinesAnswered }).map((rule) => ({
          key: `rule-run:${rule.id}`,
          section: RULES_SECTION,
          name: rule.name,
          sub: rule.sub,
          icon: <Play size={15} weight="fill" />,
          run: { ruleRun: rule.id },
        }))
      : []
    return {
      actions: [...stopRow, ...harnessRow, ...raceRow, ...sinceRow, ...layoutRow, ...layoutsRow, ...renameRow, ...setupRow, ...presetRow, ...reopenRow, ...review, ...markdown, ...jobRow, ...pages, ...actions],
      chats: chatRows,
      projectRows,
      machines: machineRows,
      harnessRows,
      ruleRows,
    }
  }, [kb, sessions, archived, projects, devices, thisDevice, capabilities, machines, raceFree, moved, panes, savedLayouts, onScreen, reviewChat, markdownChat, hasFileChanges, focusedChatId, onRenameChat, rowSetup, onNewWithSetup, rulesCanRun, automations, machinesAnswered])

  const sections = useMemo(() => {
    if (page === 'layouts') {
      // One shape per row, its pane count beside it, and a second row that asks
      // once before deleting it - the console's armed-row rule, in the row shape
      // the palette has (one control per row, the words carry the danger).
      const rows = savedLayouts.flatMap((entry) => {
        const panesText = `${entry.panes} ${entry.panes === 1 ? 'pane' : 'panes'}`
        const restore = {
          key: `layout:${entry.id}`,
          section: 'Layouts',
          name: entry.name,
          sub: panesText,
          icon: <SquaresFour size={15} />,
          run: { restoreLayout: entry },
        }
        const armed = rowIsArmed(armedLayout, entry.id)
        const remove = {
          key: `layout-del:${entry.id}`,
          section: 'Layouts',
          name: armed ? `Delete “${entry.name}” for good` : `Delete “${entry.name}”`,
          sub: armed ? 'This cannot be undone' : null,
          icon: <TrashSimple size={15} />,
          run: armed ? { layoutDelete: entry.id } : { layoutArm: entry.id },
        }
        return [restore, remove]
      })
      const shown = query.trim() ? rankMatches(query, rows, (r) => r.name) : rows
      return shown.length ? [{ label: 'Layouts', items: shown }] : []
    }
    if (page === 'caught-up') {
      // The confirmation is the page: one sentence above, and the only row left
      // is the way out.
      return [
        {
          label: 'Since your last look',
          items: [
            {
              key: 'caught:close',
              section: 'Since your last look',
              name: 'Close',
              run: { caughtClose: true },
            },
          ],
        },
      ]
    }
    if (page === 'rule-run') {
      // The answer is the page: one line above, and the only row left is the
      // way out, so Enter or Esc both end it without touching the answer.
      return [
        {
          label: 'Rules',
          items: [{ key: 'rule-run:close', section: 'Rules', name: 'Close', run: { caughtClose: true } }],
        },
      ]
    }
    if (page === 'since') {
      // A record, not a queue: the chats are in the order the module put them,
      // newest activity first, and each row says what happened last and when.
      const rows = moved.map((row) => ({
        key: `since:${row.id}`,
        section: 'Chats',
        name: row.title,
        sub: [whatHappened(row.status), whenLabel(row.updatedAt)].filter(Boolean).join(' · '),
        icon: <HarnessMark harness={row.harness} size={15} />,
        veiled: true,
        run: { sinceChat: { id: row.id, stamp: row.updatedAt } },
      }))
      const shown = query.trim() ? rankMatches(query, rows, (r) => r.name) : rows
      return shown.length ? [{ label: 'Chats', items: shown }] : []
    }
    if (page === 'stop-all') {
      // The question lives above this list; these are the two answers. The
      // safe one comes first, so the Enter that muscle memory sends keeps
      // the work rather than ending it.
      if (stopPhase === 'stopping') return []
      if (stopPhase === 'done') {
        return [
          {
            label: 'Stop everything',
            items: [{ key: 'stop:close', section: 'Stop everything', name: 'Close', run: { stopClose: true } }],
          },
        ]
      }
      return [
        {
          label: 'Stop everything',
          items: [
            { key: 'stop:keep', section: 'Stop everything', name: 'Keep them running', run: { stopKeep: true } },
            { key: 'stop:stop', section: 'Stop everything', name: 'Stop them', run: { stopGo: true } },
          ],
        },
      ]
    }
    if (page === 'stop-harness') {
      const label = HARNESS_STOP_NAME
      // While the stops land: nothing to answer, the question above says what
      // is happening. After: one way out, so the palette never closes under a
      // sentence nobody has read.
      if (harnessPhase === 'stopping') return []
      if (harnessPhase === 'done') {
        return [
          {
            label,
            items: [{ key: 'harness-stop:close', section: label, name: 'Close', run: { harnessClose: true } }],
          },
        ]
      }
      if (harnessStopId && harnessPlan) {
        // The confirm's two answers. The safe one is first for the same
        // reason stop-all puts it first: the Enter muscle memory sends keeps
        // the work.
        return [
          {
            label,
            items: [
              { key: 'harness-stop:keep', section: label, name: 'Keep them running', run: { harnessKeep: true } },
              { key: 'harness-stop:stop', section: label, name: 'Stop them', run: { harnessGo: true } },
            ],
          },
        ]
      }
      // The pick: one row per harness this computer can stop, biggest first,
      // each with what a press would end.
      return everything.harnessRows.length
        ? [
            {
              label,
              items: everything.harnessRows.map((row) => ({
                key: `harness-stop:${row.id}`,
                section: label,
                name: row.name,
                sub: runningLabel(row.count),
                icon: <HarnessMark harness={row.id} size={15} />,
                run: { harnessPick: row.id },
              })),
            },
          ]
        : []
    }
    if (page) {
      const src = kb.source(page) ?? { items: [] }
      const rows = src.items.map((item) => ({
        key: `pick:${item.id}`,
        section: src.title,
        name: item.name,
        icon: item.icon ?? null,
        selected: item.id === src.selectedId,
        run: { pick: item.id },
      }))
      const shown = query.trim() ? rankMatches(query, rows, (r) => r.name) : rows
      return shown.length ? [{ label: src.title, items: shown }] : []
    }
    if (!query.trim()) {
      const byId = new Map(everything.chats.map((c) => [c.run.chat, c]))
      const recent = recentChatIds.map((id) => byId.get(id)).filter(Boolean).slice(0, 5)
      // The review belongs in the suggestions when there is something to
      // review: it is the one action on this list the user cannot reach
      // anywhere else yet.
      const suggested = [
        'action:new-chat',
        'action:changes',
        'page:model',
        'action:view-settings',
        'action:cheat-sheet',
      ]
        .map((k) => everything.actions.find((a) => a.key === k))
        .filter(Boolean)
      return [
        ...(recent.length ? [{ label: 'Recent', items: recent }] : []),
        ...(suggested.length ? [{ label: 'Suggestions', items: suggested }] : []),
      ]
    }
    const all = [...everything.actions, ...everything.chats, ...everything.projectRows, ...everything.machines, ...everything.ruleRows, ...tabRows]
    // The score ranks; the tie rule only settles two rows that scored the
    // same - a label the query begins, then a chat over an action.
    const ranked = rankMatches(query, all, (r) => r.name, tieRule(query, (row) => row.section === 'Chats'))
    return SECTION_ORDER.map((label) => {
      const forSection = ranked.filter((r) => r.section === label)
      const items = forSection.slice(0, SECTION_CAP)
      // A capped group says so in its own last row: with fifty chats or ten
      // rules, the ninth match used to be dropped in silence and the field
      // looked broken rather than narrow. The row carries no action - it is a
      // fact, not a link - so Enter on it does nothing.
      if (forSection.length > SECTION_CAP) {
        const rest = forSection.length - SECTION_CAP
        items.push({
          key: `more:${label}`,
          section: label,
          name: `and ${rest} more`,
          sub: 'Refine the search to narrow it',
          run: null,
        })
      }
      return { label, items }
    }).filter((s) => s.items.length)
  }, [page, query, everything, tabRows, recentChatIds, kb, stopPhase, moved, harnessStopId, harnessPhase, savedLayouts, armedLayout])

  const flat = sections.flatMap((s) => s.items)

  useEffect(() => setCursor(0), [query, page])
  // A look at another list is a decision to keep every layout: the armed row
  // never survives a page change or a new search.
  useEffect(() => setArmedLayout(clearRow()), [page, query])
  useEffect(() => {
    listRef.current?.querySelector('.kb-item.cursor')?.scrollIntoView({ block: 'nearest' })
  }, [cursor])

  // The stop itself, per chat, through the action a single chat already
  // uses (`stopChat` calls the daemon's own stop route; no new route, no
  // mesh call). A chat that is already gone is skipped rather than failed:
  // the daemon answers 404 for a session it no longer has, which means the
  // work this command was about has ended anyway. Both stops count the same
  // way, so this is one loop, not two.
  const stopChats = async (chats) => {
    let stopped = 0
    let gone = 0
    let failed = 0
    for (const chat of chats) {
      try {
        await stopChat(chat.id)
        stopped += 1
      } catch (err) {
        if (sessionGone(err)) gone += 1
        else failed += 1
      }
    }
    return { stopped, gone, failed }
  }

  const runStopAll = async () => {
    const plan = stopPlan
    if (!plan) return
    setStopPhase('stopping')
    const { stopped, gone, failed } = await stopChats(plan.stoppable)
    setStopResult(stopSummary({ stopped, elsewhere: plan.elsewhere.length, gone, failed }))
    setStopPhase('done')
  }

  // The same loop over one harness's chats, with the sentence that names it.
  const runStopHarness = async () => {
    const plan = harnessPlan
    const name = harnessName(harnessStopId)
    if (!plan || !harnessStopId) return
    setHarnessPhase('stopping')
    const counts = await stopChats(plan.stoppable)
    setHarnessResult(harnessSummary({ name, ...counts, elsewhere: plan.elsewhere.length }))
    setHarnessPhase('done')
  }

  const choose = (row) => {
    if (!row) return
    // A row with no action is a fact, not a control: the "and N more" row a
    // capped group ends with. Enter on it must do nothing rather than throw.
    if (!row.run) return
    const r = row.run
    if (r.tab) {
      // The strip's own switch: the store's active tab is what every surface
      // reads (the strip, the address bar, the page the pane shows), so this is
      // the same call a click on the strip makes, and the palette closes
      // because the place the person chose is the browser behind it.
      setState(() => ({ active: r.tab }))
      onClose()
      return
    }
    if (r.ruleRun) {
      // The rule keeps the palette open while the daemon is asked: the answer
      // is the one line under the field, first "Running ...", then "Started."
      // or the daemon's own refusal. The run itself is the Automations view's
      // Run now, lent through rule-run.js's seam, so this branch carries no
      // run path of its own - only the two things the palette owns: the line
      // and the wait.
      setRuleRunState({ phase: 'running', name: row.name })
      setPage('rule-run')
      runRule(r.ruleRun).then((result) => {
        setRuleRunState({ phase: result.ok ? 'started' : 'refused', name: row.name, error: result.error })
      })
      return
    }
    if (r.markdown) {
      // The copy keeps the palette open: the answer is one line, and a palette
      // that closed to say it would need somewhere else to say it. The chat
      // is read from this window's own state, so nothing is fetched and no
      // export dialog exists.
      const chat = chats?.[r.markdown] ?? null
      const file = chat ? chatMarkdown(chat) : null
      if (!file) {
        setCopied('Nothing to copy in this chat.')
        return
      }
      const write = navigator.clipboard?.writeText?.(file.text)
      if (!write) {
        setCopied('Could not copy: this window has no clipboard.')
        return
      }
      write
        .then(() => setCopied(copiedLine(file)))
        // A refused clipboard write is a failure like any other, so it goes
        // through the one rule rather than pasting a caught message here.
        .catch((err) => setCopied(errorCopy(err)))
      return
    }
    if (r.saveLayout) {
      // The name is asked in the family's dialog, not here: the palette's one
      // field is a search, and a name typed into it would be a second kind of
      // enter. The palette closes first so the dialog takes the keyboard.
      onClose()
      requestAnimationFrame(() => onSaveLayout?.())
      return
    }
    if (r.layouts) {
      setPage('layouts')
      setQuery('')
      setArmedLayout(clearRow())
      setLayoutNote(null)
      inputRef.current?.focus()
      return
    }
    if (r.layoutArm) {
      // The console's armed-row rule, in the row shape this list has: one row
      // armed at a time, and the second press is the one that deletes.
      setArmedLayout((current) => armRow(current, r.layoutArm))
      setLayoutNote(null)
      inputRef.current?.focus()
      return
    }
    if (r.layoutDelete) {
      const next = withoutLayout(savedLayouts, r.layoutDelete)
      saveLayouts(next)
      setSavedLayouts(next)
      setArmedLayout(clearRow())
      inputRef.current?.focus()
      return
    }
    if (r.restoreLayout) {
      // Nothing is applied until the whole shape fits: a window that cannot
      // hold it says why instead of drawing half of a four-pane layout.
      const plan = restoreShape(r.restoreLayout, { currentChat, capacity: paneCapacity })
      if (!plan.ok) {
        setLayoutNote(capacityLine(plan.refusal) || 'That layout cannot open in this window.')
        inputRef.current?.focus()
        return
      }
      onClose()
      requestAnimationFrame(() => onRestoreLayout?.(plan))
      return
    }
    if (r.markAll) {
      // One pass, one write: exactly the chats the page above lists, each
      // recorded at its own activity, and the same storage the sidebar reads.
      // It asks nothing, and the page that follows says what happened.
      const { seen: next, count } = markAllSeen(moveRows, seen)
      if (count > 0) {
        setSeen(next)
        storeSeen(next)
      }
      setPage('caught-up')
      setQuery('')
      inputRef.current?.focus()
      return
    }
    if (r.caughtClose) {
      onClose()
      return
    }
    if (r.since) {
      // A page, like the stop-everything question: the list is the point, so
      // opening it stays inside the palette rather than closing to show it.
      setPage('since')
      setQuery('')
      inputRef.current?.focus()
      return
    }
    if (r.sinceChat) {
      // Looking at the chat is the look: record it with unread.js's own rule,
      // so the sidebar's mark and this page cannot disagree about what read
      // means. The record never goes backwards, and storage is the same one
      // the sidebar writes.
      const next = recordSeen(seen, r.sinceChat.id, r.sinceChat.stamp)
      if (next !== seen) {
        setSeen(next)
        storeSeen(next)
      }
    }
    if (r.stopAll) {
      // The question is asked inside the palette, on the numbers the list
      // shows: closing to ask elsewhere would lose the count and the names
      // the user is deciding on. With nothing stoppable, the command skips
      // the question and says what kept running.
      const targets = stopTargets(sessions ?? [], { thisDevice })
      setStopPlan(targets)
      setStopResult(
        targets.stoppable.length === 0 ? stopSummary({ stopped: 0, elsewhere: targets.elsewhere.length }) : null,
      )
      setStopPhase(targets.stoppable.length === 0 ? 'done' : 'asking')
      setPage('stop-all')
      setQuery('')
      inputRef.current?.focus()
      return
    }
    if (r.stopKeep) {
      back()
      return
    }
    if (r.stopGo) {
      runStopAll()
      return
    }
    if (r.stopClose) {
      onClose()
      return
    }
    if (r.stopHarnessPage) {
      // The page opens inside the palette, like stop-everything's question:
      // the list of harnesses is the point, and closing to show it would lose
      // the search the person already typed.
      setPage('stop-harness')
      setHarnessStopId(null)
      setHarnessPlan(null)
      setHarnessPhase(null)
      setHarnessResult(null)
      setQuery('')
      inputRef.current?.focus()
      return
    }
    if (r.harnessPick) {
      // Choosing a harness asks one question about that harness, on the plan
      // the list was showing (so the count cannot move between the ask and
      // the press). A harness whose chats all ended in the meantime answers
      // with what happened instead of asking about nothing.
      const picked = harnessStops(sessions ?? [], { thisDevice }).find((row) => row.id === r.harnessPick)
      const name = picked?.name ?? harnessName(r.harnessPick)
      const plan = stopTargets((sessions ?? []).filter((s) => s.harness === r.harnessPick), { thisDevice })
      setHarnessStopId(r.harnessPick)
      setHarnessPlan(plan)
      setHarnessResult(
        plan.stoppable.length === 0 ? harnessSummary({ name, elsewhere: plan.elsewhere.length }) : null,
      )
      setHarnessPhase(plan.stoppable.length === 0 ? 'done' : 'asking')
      setQuery('')
      return
    }
    if (r.harnessKeep) {
      // One step back, to the harness list: the question here is about one
      // harness, and someone who says no to it may want another one.
      setHarnessStopId(null)
      setHarnessPlan(null)
      setHarnessPhase(null)
      setHarnessResult(null)
      setQuery('')
      return
    }
    if (r.harnessGo) {
      runStopHarness()
      return
    }
    if (r.harnessClose) {
      onClose()
      return
    }
    if (r.page) {
      setPage(r.page)
      setQuery('')
      inputRef.current?.focus()
      return
    }
    if (r.pick !== undefined) {
      kb.source(page)?.choose(r.pick)
      onClose()
      return
    }
    if (r.savePreset) {
      // The naming dialog is drawn over this list, and its state lives here,
      // so the palette has to stay mounted underneath it: closing first would
      // take the dialog with it. The modal family's backdrop (z-index 100)
      // sits above the palette's own (40), so this one stacks correctly.
      setNamingPreset(r.savePreset)
      return
    }
    onClose()
    // After the palette is gone, so focus and anchors land on the app. The
    // rule editor lives in the Automations view, so the draft is staged for
    // it first and the shell is asked for that view second: the view picks
    // the draft up as it mounts.
    requestAnimationFrame(() => {
      if (r.action) kb.run(r.action)
      else if (r.changes) openChanges(r.changes)
      else if (r.race) onRace?.()
      else if (r.sinceChat) onOpenChat(r.sinceChat.id)
      else if (r.job) {
        requestJob(r.job)
        kb.run('view-automations')
      } else if (r.chat) onOpenChat(r.chat)
      else if (r.project) onOpenProject(r.project)
      else if (r.machine) onOpenMachine(r.machine)
      // The rename dialog belongs to the shell (it hosts it above every
      // surface that offers renaming), so the palette closes first and then
      // asks, the same shape as the arms above.
      else if (r.renameChat) onRenameChat?.(r.renameChat)
    // "New chat with this setup" navigates, and navigation belongs to the
    // shell: the palette closes and asks, the same shape as the rename arm.
    else if (r.newWithSetup) onNewWithSetup?.(r.newWithSetup)
    else if (r.reopenClosed) reopenClosed()
    })
  }

  const back = () => {
    setPage(null)
    setQuery('')
  }

  // The page that has two levels (the harness list, then one harness's
  // question) steps back one level before it leaves the page, so Esc and
  // Backspace mean the same thing they mean everywhere else in the palette.
  const pageBack = () => {
    if (page === 'stop-harness' && harnessStopId) {
      setHarnessStopId(null)
      setHarnessPlan(null)
      setHarnessPhase(null)
      setHarnessResult(null)
      setQuery('')
      return
    }
    back()
  }

  // Esc and the palette's own key answer from anywhere inside the dialog: Tab
  // moves focus onto a row, and the key the footer promises for leaving has to
  // keep working there. Arrows, Enter and Backspace stay with the search
  // field, so a focused row keeps its own native Enter.
  const onKeyDown = (e) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      if (page) pageBack()
      else onClose()
      return
    }
    if (comboFromEvent(e, isMac) === kb.bindings.palette) {
      // The palette's own key closes it again.
      e.preventDefault()
      onClose()
      return
    }
    if (e.target !== inputRef.current) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setCursor((c) => (flat.length ? (c + 1) % flat.length : 0))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setCursor((c) => (flat.length ? (c - 1 + flat.length) % flat.length : 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      choose(flat[cursor])
    } else if (e.key === 'Backspace' && !query && page) {
      e.preventDefault()
      pageBack()
    }
  }

  let index = -1
  const stopNames = stopPlan ? namedChats(stopPlan.stoppable) : null
  const harnessNames = harnessPlan ? namedChats(harnessPlan.stoppable) : null
  const harnessNameNow = harnessStopId ? harnessName(harnessStopId) : null
  return (
    <div
      className="kb-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div
        className="kb-palette"
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onKeyDown={onKeyDown}
      >
        <div className="kb-search">
          <MagnifyingGlass size={15} />
          {page && <span className="kb-crumb">{kb.source(page)?.title ?? PAGE_TITLES[page]}</span>}
          <input
            ref={inputRef}
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={page ? 'Search' : 'Type a command or search'}
            spellCheck={false}
            role="combobox"
            aria-expanded="true"
            aria-controls="kb-list"
          />
        </div>
        {copied && (
          <p className="kb-copied" role="status">
            {copied}
          </p>
        )}
        <div className="kb-list" id="kb-list" role="listbox" ref={listRef}>
          {page === 'layouts' && layoutNote && (
            // Why a shape cannot open here, said once above the list. Nothing
            // was applied, and nothing was changed to fit.
            <div className="kb-stop">
              <p className="kb-stop-question" role="status">
                {layoutNote}
              </p>
            </div>
          )}
          {page === 'rule-run' && ruleRunState && (
            // The answer to running a rule: one line, the same slot a copy or a
            // layout refusal uses. It changes in place from "Running ..." to
            // "Started." or the daemon's sentence, so nothing moves under the
            // cursor while the daemon is asked.
            <div className="kb-stop">
              <p className="kb-stop-question" role="status">
                {ruleAnswer(ruleRunState)}
              </p>
            </div>
          )}
          {page === 'caught-up' && (
            // The answer to a command that asked nothing: one sentence, and the
            // way out under it.
            <div className="kb-stop">
              <p className="kb-stop-question" role="status">
                All caught up.
              </p>
            </div>
          )}
          {page === 'since' && (
            // The count as a sentence: what the page is about, said in words
            // rather than worn as a badge.
            <div className="kb-stop">
              <p className="kb-stop-question" role="status">
                {movedLine(moved.length) || 'Nothing has moved since your last look.'}
              </p>
            </div>
          )}
          {page === 'stop-all' && (
            <div className="kb-stop">
              {stopPhase === 'asking' && stopPlan && stopNames && (
                <>
                  <p className="kb-stop-question">{stopQuestion(stopPlan.stoppable.length)}</p>
                  <div className="kb-stop-names">
                    {stopNames.rows.map((row) => (
                      <div className="kb-stop-name pv" key={row.id}>
                        {row.name}
                      </div>
                    ))}
                    {stopNames.more > 0 && <div className="kb-stop-more">and {stopNames.more} more</div>}
                  </div>
                  {stopPlan.elsewhere.length > 0 && (
                    <p className="kb-stop-aside">{elsewhereAside(stopPlan.elsewhere.length)}</p>
                  )}
                </>
              )}
              {stopPhase === 'stopping' && stopPlan && (
                <p className="kb-stop-question">{stoppingLine(stopPlan.stoppable.length)}</p>
              )}
              {stopPhase === 'done' && stopResult && (
                <p className="kb-stop-question" role="status">
                  {stopResult}
                </p>
              )}
            </div>
          )}
          {page === 'stop-harness' && (
            <div className="kb-stop">
              {harnessPhase === 'asking' && harnessPlan && harnessNames && harnessNameNow && (
                <>
                  <p className="kb-stop-question">
                    {harnessQuestion(harnessNameNow, harnessPlan.stoppable.length)}
                  </p>
                  <div className="kb-stop-names">
                    {harnessNames.rows.map((row) => (
                      <div className="kb-stop-name pv" key={row.id}>
                        {row.name}
                      </div>
                    ))}
                    {harnessNames.more > 0 && <div className="kb-stop-more">and {harnessNames.more} more</div>}
                  </div>
                  {harnessPlan.elsewhere.length > 0 && (
                    <p className="kb-stop-aside">{elsewhereAside(harnessPlan.elsewhere.length)}</p>
                  )}
                </>
              )}
              {harnessPhase === 'stopping' && harnessPlan && harnessNameNow && (
                <p className="kb-stop-question">
                  {harnessStopping(harnessNameNow, harnessPlan.stoppable.length)}
                </p>
              )}
              {harnessPhase === 'done' && harnessResult && (
                <p className="kb-stop-question" role="status">
                  {harnessResult}
                </p>
              )}
              {!harnessPhase && (
                <>
                  <p className="kb-stop-aside">{HARNESS_PICK_LINE}</p>
                  {flat.length === 0 && <p className="kb-stop-aside">{HARNESS_EMPTY_LINE}</p>}
                </>
              )}
            </div>
          )}
          {sections.map((section) => (
            <div key={section.label} className="kb-group">
              <div className="kb-group-label">{section.label}</div>
              {section.items.map((row) => {
                index += 1
                const i = index
                return (
                  <button
                    key={row.key}
                    type="button"
                    role="option"
                    aria-selected={i === cursor}
                    className={`kb-item${i === cursor ? ' cursor' : ''}`}
                    onMouseMove={() => i !== cursor && setCursor(i)}
                    onClick={() => choose(row)}
                  >
                    {row.icon && <span className="kb-icon">{row.icon}</span>}
                    <span className={row.veiled ? 'kb-name pv' : 'kb-name'}>{row.name}</span>
                    {row.sub && <span className={row.subVeiled ? 'kb-sub pv' : 'kb-sub'}>{row.sub}</span>}
                    {row.selected && <Check size={13} className="kb-check" />}
                    {row.keys && <kbd className="kb-keys">{row.keys}</kbd>}
                  </button>
                )
              })}
            </div>
          ))}
          {flat.length === 0 && page !== 'stop-all' && page !== 'stop-harness' && (
            <div className="kb-empty">
              {/* The layouts page says what is missing in its own words, the
                  same sentence the pane header's menu uses (layouts.js), not
                  the generic line for a page that has nothing at all. */}
              {query.trim()
                ? `Nothing matches “${query.trim()}”.`
                : page === 'layouts'
                  ? SAVED_LAYOUTS_NONE
                  : 'Nothing to show here yet.'}
            </div>
          )}
        </div>
        <div className="kb-foot">
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> move
          </span>
          <span>
            <kbd>Enter</kbd> {page ? 'choose' : 'open'}
          </span>
          <span>
            <kbd>Esc</kbd> {page ? 'back' : 'close'}
          </span>
        </div>
      </div>
      {/* Drawn over the palette (its backdrop is z-index 100 against this
          one's 40), which is why the naming state lives in this component:
          the row below sets it without closing the list. */}
      {namingPreset && (
        <PresetNameModal
          preset={namingPreset}
          onSave={(name) => savePreset({ ...namingPreset, name })}
          onClose={() => setNamingPreset(null)}
        />
      )}
    </div>
  )
}
