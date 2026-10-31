import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ArrowRight, ArrowUp, House, FolderOpen, Lightning, Plus, Star, X } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { usePrivacyMode } from './privacy.js'
import { useHint, useModelSwitches, usePaletteSource, useShortcut } from './keyboard.jsx'
import FolderPickerModal from './FolderPickerModal.jsx'
import { machinePickerItems, platformLabel, signedIn } from './cloud.js'
import { HarnessMark } from './brand-marks.jsx'
import SignInModal from './SignInModal.jsx'
import HarnessManagerModal from './HarnessManagerModal.jsx'
import { EndpointManagerModal } from './EndpointManager.jsx'
import { useConsole } from './state/console.jsx'
import { harnessName } from './harness-names.js'
import { findModel, modelDisplayName, prettyModelName } from './model-display.js'
import { modelMaker } from './model-maker.js'
import { ModelMark } from './model-marks.jsx'
import { SIGNIN_HARNESSES, pickable } from './harness-manager.js'
import { loadDefaults } from './defaults.js'
import {
  NAME_LIMIT,
  defaultPreset,
  loadPresets,
  markDefault,
  onPresetsChanged,
  presetFromState,
  presetSummary,
  removePreset,
  replacePreset,
  settlePreset,
  storePresets,
  suggestedPresetName,
} from './presets.js'
import { armRow, clearRow, rowIsArmed } from './row-confirm.js'
import { useArmedRow } from './use-armed-row.js'
import { projectName } from './projects.js'
import { carrySetup, droppedLine, restoreFromCatalog } from './harness-carry.js'
import { keepLine, keepSelection } from './composer-selection.js'
import { clashes } from './plugins.js'
import { spotAtBox } from './picker-anchor.js'
import { greetingAt, greetingForNewChat, nextGreetingIndex } from './greetings.js'
import { useComposerMenus } from './useComposerMenus.jsx'
import { mentionedChats, ownCommands, takeCommands } from './composer-menus.js'
import { claimInserter } from './composer-insert.js'
import { isMobile } from './platform.js'
import './newchat.css'
import './composer.css'
import { errorCopy } from './error-copy.js'
import { messageBound } from './send-bound.js'


// The harnesses whose own login command the daemon can relay - the
// sign-in prompt offers the relay only there; the others say their own
// honest words.


const EFFORT_LABELS = {
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra High',
  extrahigh: 'Extra High',
  max: 'Max',
  maximum: 'Max',
}

export function effortLabel(tier) {
  if (!tier) return 'Effort'
  const key = tier.toLowerCase().replace(/[\s_-]+/g, '')
  return EFFORT_LABELS[key] ?? tier.charAt(0).toUpperCase() + tier.slice(1)
}

export function FadingLabel({ text, className }) {
  const shown = useRef(text)
  const [outgoing, setOutgoing] = useState(null)
  const [, bump] = useState(0)
  useEffect(() => {
    if (text === shown.current) return
    setOutgoing(shown.current)
    shown.current = text
    bump((n) => n + 1)
    const id = setTimeout(() => setOutgoing(null), 320)
    return () => clearTimeout(id)
  }, [text])
  return (
    <span className={`nc-label-swap${className ? ` ${className}` : ''}`}>
      {outgoing != null && <span className="nc-label-out">{outgoing}</span>}
      <span className="nc-label-in" key={shown.current}>
        {shown.current}
      </span>
    </span>
  )
}

export function EffortSlider({
  tiers,
  value,
  spot,
  fast,
  fastSupported,
  leaving,
  onEffort,
  onFast,
  place = 'below',
}) {
  const index = Math.max(0, tiers.indexOf(value ?? tiers[0]))
  const last = Math.max(1, tiers.length - 1)
  const snapped = tiers.length < 2 ? 0 : (index / last) * 100
  const trackRef = useRef(null)
  const draggingRef = useRef(false)
  const [dragging, setDragging] = useState(false)
  const [live, setLive] = useState(snapped)
  const [specks, setSpecks] = useState([])
  useEffect(() => {
    if (!draggingRef.current) setLive(snapped)
  }, [snapped])
  useEffect(() => {
    if (!fast) {
      setSpecks([])
      return
    }
    const count = 3 + index
    setSpecks(
      Array.from({ length: count }, (_, id) => ({
        id,
        y: 6 + Math.random() * 88,
        bob: (Math.random() * 8 - 4).toFixed(1),
      })),
    )
  }, [fast, index])
  const pctFrom = (e) => {
    const rect = trackRef.current?.getBoundingClientRect()
    if (!rect || rect.width <= 0) return snapped
    const x = Math.min(Math.max(0, e.clientX - rect.left), rect.width)
    return (x / rect.width) * 100
  }
  const release = (e) => {
    if (!draggingRef.current) return
    draggingRef.current = false
    setDragging(false)
    const pct = pctFrom(e)
    const next = Math.min(last, Math.max(0, Math.round((pct / 100) * last)))
    onEffort(tiers[next])
  }
  const panel = (
    <div
      className={`nc-effort${fast ? ' is-fast' : ''}${leaving ? ' is-out' : ''}${place === 'above' ? ' is-above' : ''}`}
      role="dialog"
      aria-label="Effort"
      style={spot ? { left: spot.left, top: spot.top } : undefined}
    >
      <div className="nc-effort-head">
        {fastSupported ? (
          <button
            type="button"
            className={`nc-effort-fast${fast ? ' on' : ''}`}
            aria-pressed={fast}
            title="Fast"
            onClick={onFast}
          >
            <Lightning size={15} weight={fast ? 'fill' : 'regular'} className="nc-effort-bolt" />
          </button>
        ) : (
          <span />
        )}
        <FadingLabel className="nc-effort-name" text={effortLabel(value ?? tiers[index])} />
      </div>
      <div
        className={`nc-effort-slider${dragging ? ' is-drag' : ''}${fast ? ' is-fast' : ''}`}
        ref={trackRef}
        onPointerDown={(e) => {
          draggingRef.current = true
          setDragging(true)
          setLive(pctFrom(e))
          e.currentTarget.setPointerCapture(e.pointerId)
        }}
        onPointerMove={(e) => {
          if (!draggingRef.current) return
          setLive(pctFrom(e))
        }}
        onPointerUp={release}
        onPointerCancel={() => setDragging(false)}
      >
        <span className="nc-effort-track" />
        <span className="nc-effort-fill" style={{ width: `calc(11px + (100% - 22px) * ${live / 100})` }} />
        <span className="nc-effort-specks" style={{ width: `calc(11px + (100% - 22px) * ${live / 100})` }}>
          <span className="nc-effort-speck-lane">
            {specks.map((speck) => (
              <span
                key={speck.id}
                className="nc-effort-speck"
                style={{
                  top: `${speck.y}%`,
                  animationDelay: `${-(speck.id * 2.4) / specks.length}s`,
                  '--bob': `${speck.bob}px`,
                }}
              />
            ))}
          </span>
        </span>
        {tiers.map((tier, i) => {
          const at = tiers.length < 2 ? 0 : i / last
          return (
            <span
              key={tier}
              className={`nc-effort-dot${at * 100 <= live + 0.5 ? ' on' : ''}`}
              style={{ left: `calc(11px + (100% - 22px) * ${at})` }}
            />
          )
        })}
        <span className="nc-effort-thumb" style={{ left: `calc(11px + (100% - 22px) * ${live / 100})` }} />
      </div>
    </div>
  )
  return createPortal(panel, document.body)
}

export default function NewChat({
  onOpen,
  mode = 'local',
  addProjectOpen = false,
  onAddProjectDone,
  presetProject = null,
  setupSeed = null,
  focused = true,
  onOpenPlugins = null,
  topRight = null,
}) {
  const privacy = usePrivacyMode()
  const {
    connection, capabilities, chats, createChat, modelsFor, endpointsFor, refreshEndpoints,
    endpointModelsFor, devices, thisDevice,
    capabilitiesFor, apiFor, projects, addProject, plugins, cloud, renameChat,
  } = useConsole()
  // Plugins of one name both on: said here too, where chats start.
  const clashing = mode === 'local' ? clashes(plugins?.installed) : []
  // In Cloud a chat can run on this computer or one of the account's
  // machines; in Local it always runs here.
  const [target, setTarget] = useState(null)
  const [targetCaps, setTargetCaps] = useState(null)
  const [picking, setPicking] = useState(false)
  const [addProjectError, setAddProjectError] = useState(null)
  const cloudMode = mode === 'cloud'
  // A phone runs no chats: its list is the machines alone.
  const machineOptions = cloudMode ? machinePickerItems(devices, thisDevice, { phone: isMobile }) : []
  const targetItem = machineOptions.find((m) => m.id === target) ?? null
  const targetOffline = Boolean(target && targetItem && !targetItem.online)
  const [defaults] = useState(() => loadDefaults())
  // The setups this person saved, and the one New chat opens on. Both are
  // read once, at mount: they are local storage reads. A preset saved while
  // this screen is already on screen (a second pane, or a chat saved from the
  // palette) arrives on presets.js's own channel instead, so the chip appears
  // without a reload.
  const [presets, setPresets] = useState(() => loadPresets())
  useEffect(() => onPresetsChanged((list) => setPresets(list)), [])
  const [openingPreset] = useState(() => defaultPreset(loadPresets()))
  // The preset waiting for a harness switch to finish: its slots can only
  // be settled once the new harness has answered with its model list.
  const pendingPreset = useRef(openingPreset ?? null)
  // A saved setup that no longer matches this machine settles to what is available here
  // without narrating the difference: the founder's call, and the right one. The two
  // notes below stay, because they are about the chat in front of you rather than about
  // a comparison with something else.
  // The chip being renamed in place: { id: 'new' | preset id, draft }.
  // 'new' is the Save chip before it becomes a preset.
  const [presetEditing, setPresetEditing] = useState(null)
  // The chip whose delete control is armed: one row at a time, and the
  // same helper the plugins and MCP panes use. The helper's browser half comes
  // with it: Escape disarms (DESIGN.md's "Escape or a click elsewhere keeps"),
  // and so does losing the control. Without the hook this row was armed until it
  // was pressed again: no key and no click anywhere else took the arm away.
  const [armedPreset, setArmedPreset] = useState(null)
  const presetArmWatch = useArmedRow(armedPreset, setArmedPreset)
  const [text, setText] = useState('')
  // A new chat opens on the next sentence. While the box stays empty the
  // line keeps moving, so a page left sitting does not freeze on one greeting.
  const [greetingIndex, setGreetingIndex] = useState(() => greetingForNewChat())
  const [greetingOn, setGreetingOn] = useState(true)
  useEffect(() => {
    if (text.trim()) {
      setGreetingOn(true)
      return
    }
    const idle = setTimeout(() => setGreetingOn(false), 5 * 60 * 1000)
    return () => clearTimeout(idle)
  }, [text, greetingIndex])
  useEffect(() => {
    if (greetingOn || text.trim()) return
    const swap = setTimeout(() => {
      setGreetingIndex((i) => nextGreetingIndex(i))
      setGreetingOn(true)
    }, 320)
    return () => clearTimeout(swap)
  }, [greetingOn, text])
  // The project the chat runs in: one of the registered projects, or
  // null - a chat that belongs to no project and runs in the home folder.
  const [projectId, setProjectId] = useState(null)
  const [picker, setPicker] = useState(null)
  const [harnessLeaving, setHarnessLeaving] = useState(false)
  const [modelLeaving, setModelLeaving] = useState(false)
  const [effortLeaving, setEffortLeaving] = useState(false)
  const [permissionLeaving, setPermissionLeaving] = useState(false)
  const permissionLeavingRef = useRef(false)
  const effortLeavingRef = useRef(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const [harness, setHarness] = useState(() => openingPreset?.harness ?? loadDefaults().harness)
  const [model, setModel] = useState(null)
  const [models, setModels] = useState([])
  const [modelsPending, setModelsPending] = useState(true)
  // Custom endpoints on the machine the chat will run on: the records,
  // the models each enabled one lists today, and this chat's own pick
  // (null = the harness's machine default, if any, else its provider).
  const [endpoints, setEndpoints] = useState([])
  const [endpointModels, setEndpointModels] = useState([])
  const [endpointId, setEndpointId] = useState(null)
  const [manageModels, setManageModels] = useState(false)
  const [customDraft, setCustomDraft] = useState('')
  const [fast, setFast] = useState(false)
  const [signInFor, setSignInFor] = useState(null)
  const [managingHarnesses, setManagingHarnesses] = useState(false)

  // On a phone a chat always runs on a machine: the first online one until
  // the user picks another.
  const firstMachine = machineOptions.find((m) => m.online)?.id ?? machineOptions[0]?.id ?? null
  useEffect(() => {
    if (isMobile && cloudMode && !machineOptions.some((m) => m.id === target) && firstMachine) {
      setTarget(firstMachine)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [firstMachine])

  // The sidebar's + opens the same Add project flow the home screen's
  // button opens - one way in, whichever surface asked.
  useEffect(() => {
    if (addProjectOpen) setPicking(true)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [addProjectOpen])

  // The command palette's project row: New chat, already in that project.
  // The palette lists this computer's projects, so the chat runs here.
  useEffect(() => {
    if (!presetProject) return
    setTarget(null)
    setProjectId(presetProject.id)
    // Straight to typing: the project is chosen, the message is next.
    requestAnimationFrame(() => inputRef.current?.focus())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presetProject?.nonce])

  // "New chat with this setup": the seed waits in the preset slot, because
  // both a saved preset and a setup copied from a chat have to be settled
  // against the model list the harness answers with a moment later (the
  // settlePreset branch below). The nonce makes a second press apply again,
  // and the model effect below re-runs on it so a same-harness seed is settled
  // too, rather than waiting for a fetch that would never come.
  const seedApplied = useRef(null)
  useEffect(() => {
    if (!setupSeed?.nonce || seedApplied.current === setupSeed.nonce) return
    seedApplied.current = setupSeed.nonce
    pendingPreset.current = setupSeed
    if (setupSeed.harness) setHarness(setupSeed.harness)
    requestAnimationFrame(() => inputRef.current?.focus())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setupSeed?.nonce])

  // The registered projects on the machine the chat will run on. A
  // project belongs to the machine that owns its folder, so the list
  // follows the target.
  const [targetProjects, setTargetProjects] = useState([])
  // Whether the list under the picker is the daemon's answer or the last one this window
  // had: a failed fetch used to empty it, so a machine whose relay was still coming up
  // looked like a machine with no projects, and a chat started in the home folder
  // although the machine had one.
  const [projectsFailed, setProjectsFailed] = useState(false)
  useEffect(() => {
    let cancelled = false
    // Not while the daemon is down: the fetch would only fail and blank
    // the picker for the whole reconnect window.
    if (connection.state !== 'connected') return
    apiFor(target)
      .projects()
      .then((list) => {
        if (cancelled) return
        setTargetProjects(list ?? [])
        setProjectsFailed(false)
      })
      .catch(() => {
        // The last good list stays on screen; the picker says it may be stale.
        if (!cancelled) setProjectsFailed(true)
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, connection.state, projects])

  // The mode switch keeps what still means something where the person is
  // going. A chat in Local runs on this computer, so a target machine cannot
  // survive the trip back - but the project can, and a glance at Cloud must
  // not cost either of them. What is dropped is named once, here, rather than
  // vanishing from the row.
  const [selectionNote, setSelectionNote] = useState(null)
  useEffect(() => {
    const { kept, dropped } = keepSelection(
      { projectId, target },
      {
        mode,
        // Cloud's device list arrives on the cloud poll: until the cloud state
        // itself has answered, the list is unknown (null), not empty, and a
        // machine the person chose is kept rather than declared lost. Local
        // offers no machine choice at all.
        machines: cloudMode ? (cloud == null ? null : machineOptions) : [],
        // In Cloud the project list belongs to the target and is read after
        // the switch: unknown, so the pick is kept and the row resolves.
        projects: cloudMode ? null : projects,
      },
    )
    setTarget(kept.target)
    setProjectId(kept.projectId)
    setSelectionNote(keepLine(dropped))
    // Only a mode change runs this: the lists arriving later must not reset a
    // choice the person has since made.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode])

  // A machine's own harnesses, fetched when a chat aims at it. The first
  // answer can be empty or fail - the relay link is still coming up, or
  // the machine is still scanning for its harnesses - so the fetch keeps
  // trying (backing off to every 15s) until the machine reports some, and
  // starts over whenever the machine comes back online.
  const targetOnline = Boolean(targetItem?.online)
  useEffect(() => {
    let cancelled = false
    let timer = null
    let wait = 1500
    setTargetCaps(null)
    if (!target || connection.state !== 'connected' || !targetOnline) return
    const attempt = async () => {
      let caps = null
      try {
        caps = await capabilitiesFor(target)
      } catch {
        caps = null
      }
      if (cancelled) return
      if (caps) setTargetCaps(caps)
      if (!(caps?.harnesses ?? []).some((h) => h.available)) {
        timer = setTimeout(attempt, wait)
        wait = Math.min(wait * 2, 15000)
      }
    }
    attempt()
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, targetOnline, connection.state])

  // Only what the machine reports installed - no dead UI.
  const available = ((target ? targetCaps : capabilities)?.harnesses ?? []).filter(pickable)
  const currentHarness = available.find((h) => h.id === harness) ?? available[0] ?? null
  // The picked harness's own sign-in fact: a harness that isn't signed in
  // says so right here - the task is about to run on it.
  const usingApiKey = currentHarness?.using_api_key === true && currentHarness.signed_in !== true
  const needsSignIn = currentHarness && currentHarness.signed_in === false && !usingApiKey

  // The model slot: filled where the harness advertises, reserved
  // (not rendered) where it doesn't. Every one of these choices belongs to a
  // harness (and its catalog), so a setup that changes - a different harness,
  // a different machine - keeps what the new setup can actually carry and
  // names the rest, once, in the composer's own line. A reconnect changes
  // nothing about the setup, so it keeps everything.
  const [carryNote, setCarryNote] = useState(null)
  const setupAtMount = useRef({ harness: currentHarness?.id ?? null, target })
  const pendingCarry = useRef(null)
  const droppedAtSwitch = useRef([])
  // The first run is not a switch: it is the moment the saved defaults become
  // the preselect, and the harness may still be arriving then (null -> id is
  // the capabilities landing, not the person choosing).
  const firstRun = useRef(true)
  useEffect(() => {
    let cancelled = false
    const harnessId = currentHarness?.id ?? null
    const before = setupAtMount.current
    setupAtMount.current = { harness: harnessId, target }
    const changed = before.harness !== harnessId || before.target !== target
    if (firstRun.current || (before.harness == null && harnessId != null)) {
      firstRun.current = false
      const defaults = loadDefaults()
      setModel(harnessId === defaults.harness ? defaults.model : null)
      setPermission(null)
    } else if (!changed) {
      // Reconnect, or the same setup re-asked: nothing the person chose is
      // invalid because a socket dropped. Only the catalogs are read again.
    } else {
      const { kept, dropped } = carrySetup(
        { model, permission },
        {
          // A machine change keeps this harness's catalog: the same rows, read
          // from the other machine. A harness change has no catalog yet.
          models: before.harness === harnessId ? models : null,
          permissionModes: currentHarness?.permission_modes ?? [],
        },
      )
      const also = [...(fast ? ['fast'] : []), ...(endpointId ? ['endpoint'] : [])]
      setModel(kept.model)
      setPermission(kept.permission)
      pendingCarry.current = dropped.includes('model') ? model : null
      droppedAtSwitch.current = [...dropped, ...also]
      setCarryNote(droppedLine(droppedAtSwitch.current, currentHarness?.name ?? harnessId))
    }
    setEndpointId(null)
    setFast(false)
    if (!currentHarness) {
      setModels([])
      setModelsPending(false)
      return
    }
    setModelsPending(true)
    modelsFor(currentHarness.id, target).then((list) => {
      if (!cancelled) {
        setModels(list ?? [])
        setModelsPending(false)
        // The catalog is the real answer about the model: one the setup
        // change had to drop comes back if this harness carries the same row,
        // and one it no longer lists is let go with the note saying so.
        const back = restoreFromCatalog(pendingCarry.current, list ?? [])
        const first = (list ?? []).find((m) => m.is_default)?.model ?? (list ?? [])[0]?.model ?? null
        if (back && !pendingPreset.current) {
          pendingCarry.current = null
          setModel(back)
          droppedAtSwitch.current = droppedAtSwitch.current.filter((key) => key !== 'model')
          setCarryNote(droppedLine(droppedAtSwitch.current, currentHarness?.name ?? harnessId))
        } else if (!pendingPreset.current && first) {
          setModel((current) =>
            current && (list ?? []).some((m) => m.model === current) ? current : first,
          )
        }
        // The opening preset, or one whose chip switched the harness, is
        // settled here: this is the first moment its model and effort can
        // be checked against what the harness actually offers today. One
        // shot either way - a preset that waited for a harness that is not
        // on this computer must not fire later on a manual switch.
        const waiting = pendingPreset.current
        if (waiting) {
          pendingPreset.current = null
          const harnessRows = ((target ? targetCaps : capabilities)?.harnesses ?? []).filter(pickable)
          const harnessIds = harnessRows.map((h) => h.id)
          // Which of those harnesses still have a fast mode: fast rides the
          // harness a preset was saved on, so a preset from a harness that is
          // gone must not turn it on here (presets.js settles that).
          const fastIds = harnessRows.filter((h) => h.fast_supported === true).map((h) => h.id)
          if (waiting.harness === currentHarness.id) {
            const { slots, notes } = settlePreset(waiting, {
              harnesses: harnessIds,
              fastSupported: fastIds,
              models: list ?? [],
              permissions: currentHarness.permission_modes ?? [],
              projects: targetProjects.map((p) => p.id),
              labels: presetLabels(waiting),
            })
            setModel(slots.model)
            setEffort(slots.effort)
            if (slots.permission) setPermission(slots.permission)
            setFast(slots.fast && currentHarness.fast_supported === true)
            if (slots.projectId) setProjectId(slots.projectId)
            // The settle notes are not shown: a saved setup that no longer matches this
            // machine settles to what is available without narrating the difference.
          } else {
            // Its harness is not the one running now: the project still
            // travels, and the note says why the rest did not.
            const { slots, notes } = settlePreset(waiting, {
              harnesses: harnessIds,
              fastSupported: fastIds,
              models: [],
              permissions: [],
              projects: targetProjects.map((p) => p.id),
              labels: presetLabels(waiting),
            })
            if (slots.projectId) setProjectId(slots.projectId)
          }
        }
      }
    }).catch(() => {
      // An unreachable daemon must not read as "this harness has no models":
      // the slot stays empty and the line says the list did not load.
      if (!cancelled) {
        setModels([])
        setModelsPending(false)
        setCarryNote('The model list did not load: the harness may still be starting.')
      }
    })
    // The custom rows ride beside the catalog: the records, then what
    // each enabled one lists today. A listing that fails stays out -
    // the catalog row is still the honest pick.
    if (currentHarness?.custom_endpoint_supported) {
      endpointsFor(currentHarness.id, target).then((rows) => {
        if (cancelled) return
        setEndpoints(rows ?? [])
        const live = (rows ?? []).filter((e) => e.enabled)
        if (!live.length) {
          setEndpointModels([])
          return
        }
        Promise.all(live.map((e) => endpointModelsFor(e.id, target).catch(() => []))).then((lists) => {
          if (cancelled) return
          setEndpointModels(lists.flat())
        })
      }).catch(() => {
        if (!cancelled) {
          setEndpoints([])
          setEndpointModels([])
        }
      })
    } else {
      setEndpoints([])
      setEndpointModels([])
    }
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentHarness?.id, connection.state, target, setupSeed?.nonce])

  const chosenProject = targetProjects.find((p) => p.id === projectId) ?? null
  // The chat's workspace is the project's folder; no project, no
  // workspace - the chat runs in the home folder on its machine.
  const workspace = chosenProject?.folders[0] ?? null

  // The composer's own bound, from the same rule the answer's 413 is explained
  // by: null while the message fits, the sentence when it does not. It counts
  // the body (the escaped text plus the request's reserve), so a newline-heavy
  // message is stopped here rather than by the daemon.
  const bound = messageBound(text)
  const canSend =
    text.trim().length > 0 && !busy && currentHarness && connection.state === 'connected'
    && !needsSignIn && !targetOffline && !bound
  // The harness's own pre-send facts, each in its own words: what its
  // config says a fresh chat runs (grok's config.toml, agy's settings,
  // claude's priority chain), then its catalog's default marker (codex's
  // isDefault, claude's `default` alias - the row its own picker marks
  // recommended). Neither, and the slot stays reserved.
  const configDefault = currentHarness?.default_model ?? null
  const catalogDefault = models.find((m) => m.is_default)?.model ?? null
  const resolvedDefault = configDefault ?? catalogDefault
  // The pre-send fact's label, in the harness's own words: the config's
  // model name where the catalog knows it, the id where it doesn't.
  const resolvedDefaultName = resolvedDefault ? modelDisplayName(models, resolvedDefault) : null
  const defaultLabel =
    resolvedDefaultName && currentHarness?.default_effort
      ? `${resolvedDefaultName} · ${currentHarness.default_effort}`
      : resolvedDefaultName
  // What is in flight here, from the one vocabulary (status-words.js): the three
  // statuses a chat is busy in, whether the row comes from the daemon or from the
  // console's own word for them.
  // The effort slot follows the model slot's rule: only where the active
  // model advertises tiers. A saved default that the active model doesn't
  // offer is dropped, not rendered - never a dead selection.
  const activeModel = model ? models.find((m) => m.model === model) : null
  const effortTiers = activeModel?.reasoning_efforts ?? []
  const [effort, setEffort] = useState(null)
  const savedEffort = effort ?? defaults.efforts?.[currentHarness?.id] ?? null
  const chosenEffort = effortTiers.includes(savedEffort) ? savedEffort : null
  // The permission slot: only where the harness has its own words (claude,
  // codex, grok). The picker offers the harness's vocabulary; the
  // harness's own default row is marked. Unset sends nothing - the
  // harness's default holds. A saved default that the harness no longer
  // offers is dropped, like the effort slot's.
  const permissionWords = currentHarness?.permission_modes ?? []
  const [permission, setPermission] = useState(null)
  const savedPermission =
    permission ?? defaults.permissions?.[currentHarness?.id] ?? null
  const chosenPermission = permissionWords.some((w) => w.w === savedPermission) ? savedPermission : null
    ? savedPermission
    : null

  // ---- presets ----
  // A preset records another moment, so it names its own harness, model and
  // project rather than the ones on screen: the labels it speaks in have to
  // come from its own fields, not from the current selection.
  const presetLabels = (preset) => {
    const row = preset.projectId ? targetProjects.find((p) => p.id === preset.projectId) ?? null : null
    return {
      harnessLabel: harnessName(preset.harness),
      modelLabel: preset.model ? prettyModelName(preset.model) : null,
      effortLabel: preset.effort,
      permissionLabel: preset.permission,
      projectLabel: row ? projectName(row) : null,
      currentHarnessLabel: currentHarness ? harnessName(currentHarness.id) : null,
    }
  }
  const chipLabel = (preset) => {
    if (preset.name) return preset.name
    const labels = presetLabels(preset)
    return suggestedPresetName({
      modelLabel: labels.modelLabel,
      harnessLabel: labels.harnessLabel,
      effort: preset.effort,
      projectLabel: labels.projectLabel,
    })
  }
  // What the Save chip would name this setup right now: what it is, in the
  // words the pickers above show ("Opus high, this repo").
  const suggestedForNow = () => {
    const row = targetProjects.find((p) => p.id === projectId) ?? null
    return suggestedPresetName({
      modelLabel: model ? (shownModelName ?? prettyModelName(model)) : null,
      harnessLabel: currentHarness ? harnessName(currentHarness.id) : null,
      effort: chosenEffort,
      projectLabel: row ? projectName(row) : null,
    })
  }
  const applyPreset = (preset) => {
    if (!preset) return
    setArmedPreset(clearRow())
    const fastIds = available.filter((h) => h.fast_supported === true).map((h) => h.id)
    const harnessThere = available.some((h) => h.id === preset.harness)
    if (!harnessThere) {
      // The harness is gone, so its model, effort and approval word have
      // nothing to attach to. The project still travels, and the note says
      // what stayed behind - never a silent partial apply.
      const { slots, notes } = settlePreset(preset, {
        harnesses: available.map((h) => h.id),
        fastSupported: fastIds,
        models: [],
        permissions: [],
        projects: targetProjects.map((p) => p.id),
        labels: presetLabels(preset),
      })
      if (slots.projectId) setProjectId(slots.projectId)
      return
    }
    if (currentHarness && preset.harness !== currentHarness.id) {
      // A harness switch resets every slot below this line, so the preset
      // waits for the new harness's model list and settles in that effect.
      pendingPreset.current = preset
      setHarness(preset.harness)
      return
    }
    const { slots, notes } = settlePreset(preset, {
      harnesses: available.map((h) => h.id),
      fastSupported: fastIds,
      models,
      permissions: permissionWords,
      projects: targetProjects.map((p) => p.id),
      labels: presetLabels(preset),
    })
    setModel(slots.model)
    setEffort(slots.effort)
    setPermission(slots.permission)
    setFast(slots.fast && currentHarness?.fast_supported === true)
    if (slots.projectId) setProjectId(slots.projectId)
  }
  // Naming a preset is one step, in place: the Save chip becomes an input
  // prefilled with what the setup is, and Enter keeps it. The ref holds the
  // editing value so Enter and the blur it causes cannot both commit and
  // save the same setup twice.
  const presetEditingRef = useRef(null)
  presetEditingRef.current = presetEditing
  const beginSave = () => {
    setArmedPreset(clearRow())
    setPresetEditing({ id: 'new', draft: suggestedForNow() })
  }
  const beginRename = (preset) => {
    setPresetEditing({ id: preset.id, draft: chipLabel(preset) })
  }
  const commitPresetName = () => {
    const editing = presetEditingRef.current
    if (!editing) return
    presetEditingRef.current = null
    setPresetEditing(null)
    const draft = editing.draft.trim().slice(0, NAME_LIMIT)
    if (!draft) return
    if (editing.id === 'new') {
      const preset = presetFromState({
        name: draft,
        harness: currentHarness?.id,
        model,
        effort: chosenEffort,
        permission: chosenPermission,
        projectId,
        fast,
      })
      if (!preset) return
      setPresets(storePresets([...presets, preset]))
      return
    }
    const row = presets.find((p) => p.id === editing.id)
    if (!row) return
    setPresets(storePresets(replacePreset(presets, { ...row, name: draft })))
  }
  const cancelPresetName = () => {
    presetEditingRef.current = null
    setPresetEditing(null)
  }
  const deletePreset = (id) => {
    setArmedPreset(clearRow())
    setPresets(storePresets(removePreset(presets, id)))
  }
  const toggleDefaultPreset = (id) => {
    setArmedPreset(clearRow())
    const already = presets.find((p) => p.id === id)?.default === true
    setPresets(storePresets(markDefault(presets, already ? null : id)))
  }
  const presetKeyDown = (e) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      e.stopPropagation()
      commitPresetName()
    } else if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      cancelPresetName()
    }
  }
  const presetNameInput = (editing) => (
    <input
      className="nc-preset nc-preset-input"
      value={editing.draft}
      onChange={(e) => setPresetEditing({ id: editing.id, draft: e.target.value })}
      onKeyDown={presetKeyDown}
      onBlur={commitPresetName}
      aria-label="Name this setup"
      maxLength={NAME_LIMIT}
      spellCheck={false}
      autoComplete="off"
      autoFocus
    />
  )
  // The model the chat runs from its first breath: the user's pick, else
  // the catalog's default where the harness's config names none. The
  // daemon resolves the config case itself at create (pairing grok's
  // default effort with it); a catalog-only default - claude's `default`
  // alias - would otherwise stay unnamed until the first turn's init
  // frame reports it.
  const startModel = model ?? (configDefault ? null : catalogDefault)

  // The model picker's custom rows: what each enabled endpoint lists
  // today, grouped under its name. A composite id carries the endpoint
  // back out (model ids never hold a newline - the daemon rejects
  // control characters).
  const customItems = endpointModels.map((m) => ({
    id: `${m.endpoint_id}\n${m.model}`,
    name: prettyModelName(m.display_name ?? m.model),
    group: m.endpoint_name,
    endpointId: m.endpoint_id,
    endpointName: m.endpoint_name,
    model: m.model,
  }))
  const modelItems = [
    ...models.map((m) => ({
      id: m.model,
      name: prettyModelName(m.display_name ?? m.model),
      resolved_model: m.resolved_model,
    })),
    ...customItems,
  ]
  const defaultEndpoint = endpoints.find((e) => e.is_default) ?? null
  const chosenEndpoint = endpointId ? endpoints.find((e) => e.id === endpointId) ?? null : null
  const endpointWord = chosenEndpoint?.name ?? (defaultEndpoint ? `Default (${defaultEndpoint.name})` : "Harness provider")
  const shownModelName = model
    ? (modelDisplayName(models, model) !== model
      ? modelDisplayName(models, model)
      : (customItems.find((m) => m.model === model)?.name ?? model))
    : defaultLabel
  const reloadEndpoints = async () => {
    if (!currentHarness?.custom_endpoint_supported) return
    const rows = (await refreshEndpoints(currentHarness.id, target).catch(() => [])) ?? []
    const live = rows.filter((e) => e.enabled)
    if (!live.length) {
      setEndpointModels([])
      return
    }
    const lists = await Promise.all(live.map((e) => endpointModelsFor(e.id, target).catch(() => [])))
    setEndpointModels(lists.flat())
  }
  const chooseModelId = (id) => {
    const custom = customItems.find((m) => m.id === id)
    if (custom) {
      setModel(custom.model)
      setEndpointId(custom.endpointId)
    } else {
      setModel(id)
      setEndpointId(null)
    }
    setEffort(null)
    leaveModel()
  }

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })
  // Permission hangs off the composer's right edge, the mirror of the model
  // list on the left, and never off the pointer.
  const leavePermission = () => {
    if (permissionLeavingRef.current) return
    permissionLeavingRef.current = true
    setPermissionLeaving(true)
    setTimeout(() => {
      permissionLeavingRef.current = false
      setPermissionLeaving(false)
      setPicker((current) => (current?.kind === 'permission' ? null : current))
    }, 160)
  }
  const openPermissionPicker = () => {
    if (picker?.kind === 'permission') {
      leavePermission()
      return
    }
    const rect = boxRef.current?.getBoundingClientRect()
    if (!rect) {
      setPicker({ kind: 'permission', x: 16, y: 16 })
      return
    }
    const spot = spotAtBox(rect)
    setPicker({
      kind: 'permission',
      x: Math.round(rect.right - 264),
      y: spot?.top ?? Math.round(rect.bottom),
    })
  }
  // The project list shares the composer box's left edge and always opens in
  // that spot. A second press on the same control closes it.
  const openProjectPicker = () => {
    if (picker?.kind === 'project') {
      setPicker(null)
      return
    }
    const box = boxRef.current?.getBoundingClientRect()
    const row = boxRef.current?.parentElement?.querySelector('.nc-below')?.getBoundingClientRect()
    if (!box) {
      setPicker({ kind: 'project', x: 16, y: 16 })
      return
    }
    setPicker({
      kind: 'project',
      x: Math.round(box.left),
      y: Math.round((row?.bottom ?? box.bottom) + 8),
    })
  }
  const leaveEffort = () => {
    if (effortLeavingRef.current) return
    effortLeavingRef.current = true
    setEffortLeaving(true)
    setTimeout(() => {
      effortLeavingRef.current = false
      setEffortLeaving(false)
      setPicker((current) => (current?.kind === 'effort' ? null : current))
    }, 160)
  }
  const toggleEffort = () => {
    if (picker?.kind === 'effort') leaveEffort()
    else if (!effortLeaving) setPicker({ kind: 'effort', x: 1, y: 1 })
  }
  const leaveModel = () => {
    if (modelLeaving) return
    setModelLeaving(true)
    setTimeout(() => {
      setModelLeaving(false)
      setPicker((current) => (current?.kind === 'model' ? null : current))
    }, 160)
  }
  const leaveHarness = (next) => {
    if (harnessLeaving) return
    setHarnessLeaving(true)
    setTimeout(() => {
      setHarnessLeaving(false)
      setPicker((current) => (current?.kind === 'harness' ? null : current))
      if (next === 'manage') setManagingHarnesses(true)
    }, 160)
  }
  useEffect(() => {
    if (picker?.kind !== 'effort') return
    const close = (e) => {
      if (e.target.closest?.('.nc-effort-wrap, .nc-effort')) return
      leaveEffort()
    }
    window.addEventListener('mousedown', close)
    return () => window.removeEventListener('mousedown', close)
  }, [picker?.kind])
  // The model list is the composer's, so it shares the box's left padding and
  // opens under the box rather than wherever the pointer happened to be.
  const openBoxPicker = (kind) => {
    if (picker?.kind === kind) {
      if (kind === 'harness') leaveHarness('close')
      else if (kind === 'model') leaveModel()
      else setPicker(null)
      return
    }
    const spot = spotAtBox(boxRef.current?.getBoundingClientRect())
    if (spot) setPicker({ kind, x: spot.left, y: spot.top })
    else setPicker({ kind, x: 16, y: 16 })
  }
  const openModelPicker = () => openBoxPicker('model')

  // The keyboard acts on the focused pane's composer (keyboard.jsx); a
  // picker that isn't on screen (no catalog, no permission words) lets
  // its key fall through.
  const boxRef = useRef(null)
  const inputRef = useRef(null)
  // The / and @ menus for the chat this screen will start: its harness,
  // its folder, the machine it will run on.
  const cloudReady = signedIn(cloud)
  const menus = useComposerMenus({
    text,
    setText,
    inputRef,
    harness: currentHarness?.id,
    workspace,
    deviceId: target,
    inChat: false,
    cloudReady,
    choicesFor: (arg) => {
      if (arg === 'model') return modelItems.map((m) => ({ id: m.id, name: m.name }))
      if (arg === 'effort') return effortTiers.map((e) => ({ id: e }))
      if (arg === 'fast') return [{ id: 'on' }, { id: 'off' }]
      if (arg === 'permission') return permissionWords.map((w) => ({ id: w.w, name: w.l }))
      return []
    },
  })
  // The browser pane's "attach to the chat" lands here: appended to the
  // draft, then the focus comes back so the user keeps writing. Claimed
  // again on focus, so the focused composer wins when panes split.
  const attach = (snippet) => {
    setText((t) => (t ? `${t}\n\n${snippet}` : snippet))
    requestAnimationFrame(() => inputRef.current?.focus())
  }
  // The claim belongs to this composer: it is released when the screen goes
  // away, so the browser pane says "no chat open" instead of attaching to a
  // composer that no longer exists. A focus takes the bus again (a split
  // pane's other composer may hold it), so the claim held here goes first.
  const releaseInserter = useRef(null)
  const takeInserter = () => {
    releaseInserter.current?.()
    releaseInserter.current = claimInserter(attach)
  }
  useEffect(() => {
    takeInserter()
    return () => {
      releaseInserter.current?.()
      releaseInserter.current = null
    }
  }, [])
  const openPickerAt = (kind) => {
    if (kind === 'model' || kind === 'harness') {
      const spot = spotAtBox(boxRef.current?.getBoundingClientRect())
      if (!spot) return false
      setPicker({ kind, x: spot.left, y: spot.top })
      return true
    }
    const el = boxRef.current?.querySelector(`[data-picker="${kind}"]`)
    if (!el) return false
    const r = el.getBoundingClientRect()
    setPicker({ kind, x: r.left + r.width / 2, y: r.top + r.height / 2 })
  }
  const chooseModel = (id) => {
    setModel(id)
    setEffort(null)
  }
  useShortcut(
    'focus-composer',
    () => {
      if (!inputRef.current) return false
      inputRef.current.focus()
    },
    focused,
  )
  // Steer with nothing running is an ordinary send: Ctrl+Enter starts the chat.
  useShortcut(
    'steer',
    () => {
      if (!canSend) return false
      start()
    },
    focused,
  )
  useShortcut('pick-model', () => openPickerAt('model'), focused)
  useShortcut('pick-effort', () => openPickerAt('effort'), focused)
  useShortcut('pick-permission', () => openPickerAt('permission'), focused)
  useShortcut('pick-harness', () => openPickerAt('harness'), focused)
  useShortcut(
    'toggle-fast',
    () => {
      if (!currentHarness?.fast_supported) return false
      setFast((v) => !v)
    },
    focused,
  )
  useModelSwitches((i) => {
    if (!models[i]) return false
    chooseModel(models[i].model)
  }, focused)
  usePaletteSource(
    'model',
    () =>
      models.length
        ? {
            title: 'Switch model',
            items: models.map((m) => ({
              id: m.model,
              name: prettyModelName(m.display_name ?? m.model),
              icon: <ModelMark maker={modelMaker(currentHarness?.id, m.model, m.resolved_model)} size={14} />,
            })),
            selectedId: model,
            choose: chooseModel,
          }
        : null,
    focused,
  )
  usePaletteSource(
    'effort',
    () =>
      effortTiers.length
        ? {
            title: 'Set effort',
            items: effortTiers.map((t) => ({ id: t, name: t })),
            selectedId: chosenEffort,
            choose: setEffort,
          }
        : null,
    focused,
  )
  usePaletteSource(
    'permission',
    () =>
      permissionWords.length
        ? {
            title: 'Set permission',
            items: permissionWords.map((w) => ({ id: w.w ?? w, name: w.l ?? w.w ?? w })),
            selectedId: chosenPermission,
            choose: setPermission,
          }
        : null,
    focused,
  )
  const hintModel = useHint('pick-model')
  const hintEffort = useHint('pick-effort')
  const hintHarness = useHint('pick-harness')
  const hintPermission = useHint('pick-permission')
  const hintFast = useHint('toggle-fast')

  const start = async () => {
    if (!canSend) return
    setBusy(true)
    setError(null)
    try {
      // OpenRemote's commands set this chat's start (model, effort, fast,
      // permissions), its name, and whether it goes on Cloud; the rest is
      // the first message.
      const { run, text: message } = takeCommands(text.trim(), ownCommands({ inChat: false, cloud: cloudReady }))
      const picked = Object.fromEntries(run.map((c) => [c.name, c.arg]))
      const fastPicked = 'fast' in picked ? (picked.fast ? picked.fast.toLowerCase() === 'on' : !fast) : fast
      if (!message) {
        // Only commands: they set this screen's pickers.
        if (picked.model) setModel(picked.model)
        if (picked.effort) setEffort(picked.effort)
        if ('fast' in picked) setFast(fastPicked)
        if (picked.permissions) setPermission(picked.permissions)
        setText('')
        return
      }
      const id = await createChat(
        message,
        workspace,
        currentHarness.id,
        picked.model ?? startModel ?? undefined,
        fastPicked || undefined,
        picked.effort ?? chosenEffort ?? undefined,
        picked.permissions ?? chosenPermission ?? undefined,
        {
          deviceId: target,
          synced: cloudMode || 'sync' in picked || 'cloud' in picked,
          mentionedChats: mentionedChats(message, chats),
          endpointId: endpointId ?? undefined,
        },
      )
      if (picked.rename) await renameChat(id, picked.rename)
      setText('')
      onOpen(id)
    } catch (err) {
      setError(errorCopy(err))
    } finally {
      setBusy(false)
    }
  }

  // The one way a folder becomes a project: pick it on the machine that
  // will own it, and it registers there. The same dialog serves both
  // surfaces - a chat's project pick and the Add project button.
  const chooseProjectFolder = async (folder) => {
    setPicking(false)
    setAddProjectError(null)
    try {
      const project = await addProject([folder], { deviceId: target })
      setProjectId(project.id)
    } catch (err) {
      // The daemon's own words - a folder that is already a project, a
      // path that is not a folder - surface right beside the flow.
      setAddProjectError(errorCopy(err))
    }
  }

  // Switching machines: a project belongs to one machine, so the choice
  // starts over.
  const chooseTarget = (id) => {
    setTarget(id)
    setProjectId(null)
    setPicker(null)
  }

  // The saved setups, as the page's own row: ghost text, no fill and no border,
  // the hover wash the only affordance, so the row reads as text until it is
  // reached for. Every control is a real button (or an input while naming), so the
  // row is Tab-reachable. It lives at the top of the page rather than above the
  // composer, where it crowded the pickers.
  const presetsRow = (
    <div className="nc-presets" role="group" aria-label="Saved setups">
        {presets.map((preset) => {
          const armed = rowIsArmed(armedPreset, preset.id)
          return (
            <span className="nc-preset-wrap" key={preset.id}>
              {presetEditing?.id === preset.id ? (
                presetNameInput(presetEditing)
              ) : (
                <button
                  type="button"
                  className={`nc-preset${preset.default ? ' on' : ''}`}
                  title={`${presetSummary(preset, presetLabels(preset))}. Double-click to rename.`}
                  onClick={() => applyPreset(preset)}
                  onDoubleClick={() => beginRename(preset)}
                >
                  {/* A preset carries the name the person gave it (and, in
                      the summary form, the project it was saved in), so it
                      blurs with their other content. */}
                  {/* The mark says which harness the setup opens in, so a
                      row of presets reads as setups rather than as names. */}
                  <HarnessMark harness={preset.harness} size={12} />
                  <span className="pv">{chipLabel(preset)}</span>
                </button>
              )}
              <button
                type="button"
                className="nc-preset-hover nc-preset-star"
                aria-pressed={preset.default === true}
                aria-label={preset.default ? 'New chats open on this setup' : 'Open new chats with this setup'}
                title={preset.default ? 'New chats open on this setup' : 'Open new chats with this setup'}
                onClick={() => toggleDefaultPreset(preset.id)}
              >
                <Star size={11} weight={preset.default ? 'fill' : 'regular'} />
              </button>
              <button
                type="button"
                className={`nc-preset-hover nc-preset-x${armed ? ' armed' : ''}`}
                aria-label={armed ? 'Press again to delete this setup' : 'Delete this setup'}
                title={armed ? 'Press again to delete this setup' : 'Delete this setup'}
                // The hook goes on only while this row is the armed one: arming
                // re-renders the same node, so a ref that is always attached
                // would never be called again and the Escape listener and the
                // focus would never arrive.
                ref={armed ? presetArmWatch.buttonRef : null}
                onBlur={armed ? presetArmWatch.onBlur : undefined}
                onClick={() => (armed ? deletePreset(preset.id) : setArmedPreset((curr) => armRow(curr, preset.id)))}
              >
                {armed ? 'Delete?' : <X size={11} weight="bold" />}
              </button>
            </span>
          )
        })}
        {presetEditing?.id === 'new' ? (
          presetNameInput(presetEditing)
        ) : (
          <button
            type="button"
            className="nc-preset nc-preset-save"
            onClick={beginSave}
            disabled={!currentHarness}
            title="Save the setup above as a preset"
          >
            <Plus size={12} weight="bold" />
            Save this setup
          </button>
        )}
      </div>
  )

  return (
    <div className="newchat">
      {/* The top of the page, not the composer: the saved setups sit on the left
          and the floating bar on the right, so the row above the greeting has room
          instead of squeezing into the box with the pickers. */}
      <div className="nc-page-head">
        {presetsRow}
        {topRight}
      </div>
      <h1 className={`nc-greeting${greetingOn ? '' : ' is-out'}`}>{greetingAt(greetingIndex)}</h1>
      {clashing.map((c) => (
        <div className="nc-signin" key={c.name}>
          <div className="nc-signin-note">
            Two plugins named {c.name} are on ({c.ids.join(' and ')}). A Claude chat may mix up their
            commands.
          </div>
          {onOpenPlugins && (
            <button className="nc-signin-btn" onClick={onOpenPlugins}>
              Plugins
            </button>
          )}
        </div>
      ))}
      {/* Nothing to run a chat on: the screen says so once, where the task
          box is, rather than leaving a composer whose send button is dead
          for a reason nobody can see. No button here - the harness picker
          under the box is the one control that starts this, and a second
          "Install" that opened something else would be the dead control
          the console does not ship. */}
      {connection.state === 'connected' && !available.length && (
        <p className="nc-first-run" key="no-agent">
          No agent is installed on this computer yet, so a chat has nowhere to run. Install one from
          the harness picker under the box; it scans this computer each time it opens.
        </p>
      )}
      <div className={`nc-box${needsSignIn || usingApiKey ? ' nc-box--auth' : ''}${fast ? ' is-fast' : ''}`} ref={boxRef}>
        {(carryNote || selectionNote) && (
          <p className="nc-preset-note" role="status">
            {carryNote ?? selectionNote}
          </p>
        )}
        {menus.element}
        <textarea
          value={text}
          onFocus={takeInserter}
          // Leaving the composer closes the / or @ list: a click somewhere
          // else in the window never moves the caret, so the caret's word
          // alone could not retire it and the menu hung over an unfocused box.
          onBlur={menus.dismiss}
          onChange={(e) => {
            setText(e.target.value)
            menus.track(e)
          }}
          onKeyUp={menus.track}
          onClick={menus.track}
          onKeyDown={(e) => {
            // The / and @ menus take their keys first.
            if (menus.onKeyDown(e)) return
            // A phone's keyboard Enter is a new line; the send button sends.
            if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey && !isMobile) {
              e.preventDefault()
              start()
            }
          }}
          ref={inputRef}
          placeholder="Describe the task"
          autoFocus={!isMobile}
          spellCheck={false}
        />
        {/* Said above the send control, where the person is looking when the
            button will not press: the same sentence the rule gives, carrying
            this message's size and the limit. */}
        {bound && (
          <p className="nc-bound" role="status">
            {bound}
          </p>
        )}
        <div className="nc-foot">
          <div className="nc-pickers">
            {modelsPending ? (
              <span className="nc-skeleton" aria-hidden="true" />
            ) : (modelItems.length > 0 || resolvedDefault) && (
              <>
                {modelItems.length > 0 ? (
                  <button
                    className="nc-meta"
                    onClick={openModelPicker}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'model'}
                    data-picker="model"
                    data-unset={!model || undefined}
                    title={hintModel('Model')}
                  >
                    {model ? shownModelName : 'Model'}
                  </button>
                ) : (
                  // No catalog to pick from, but the harness's own config
                  // says what a fresh chat runs - its words, shown as the
                  // fact it is. The wire's first-turn truth replaces it.
                  <span className="nc-meta nc-static" title="The model this harness runs, from its own config">
                    {defaultLabel}
                  </span>
                )}
                {(picker?.kind === 'model' || modelLeaving) && (
                  <PickerMenu
                    label="Model"
                    searchPlaceholder="Search models"
                    wide
                    leaving={modelLeaving}
                    items={modelItems}
                    groups={customItems.length > 0}
                    selectedId={model ?? ''}
                    onChoose={chooseModelId}
                    onClose={leaveModel}
                    anchor={{ left: picker.x, top: picker.y }}
                    renderSubline={(m) =>
                      m.group ? <span className="nc-item-sub">via {m.group}</span> : null
                    }
                    footer={
                      currentHarness?.custom_endpoint_supported ? (
                        <button
                          type="button"
                          onClick={() => {
                            setPicker(null)
                            setManageModels(true)
                          }}
                        >
                          Manage models…
                        </button>
                      ) : null
                    }
                  />
                )}
                {currentHarness?.custom_endpoint_supported && endpoints.length > 0 && (
                  <button
                    type="button"
                    className="nc-meta"
                    onClick={openPicker('endpoint')}
                    aria-haspopup="listbox"
                    aria-expanded={picker?.kind === 'endpoint'}
                    data-picker="endpoint"
                    title="Which model server this chat runs through"
                  >
                    via {endpointWord}
                  </button>
                )}
                {picker?.kind === 'endpoint' && (
                  <PickerMenu
                    label="Endpoint"
                    searchPlaceholder="Search endpoints"
                    items={[
                      {
                        id: '',
                        name: defaultEndpoint ? `Harness default (${defaultEndpoint.name})` : 'Harness provider',
                      },
                      ...endpoints.filter((e) => e.enabled).map((e) => ({ id: e.id, name: e.name })),
                    ]}
                    groups={null}
                    selectedId={endpointId ?? ''}
                    onChoose={(id) => {
                      setEndpointId(id || null)
                      setPicker(null)
                    }}
                    onClose={() => setPicker(null)}
                    anchor={{ left: picker.x, top: picker.y }}
                    renderSubline={(item) =>
                      item.id === '' ? (
                        <span className="nc-item-sub">
                          {defaultEndpoint ? "This harness's own default" : "This harness's own provider"}
                        </span>
                      ) : null
                    }
                    footer={
                      <button
                        type="button"
                        onClick={() => {
                          setPicker(null)
                          setManageModels(true)
                        }}
                      >
                        Manage models…
                      </button>
                    }
                  />
                )}
                <span className="nc-via">via</span>
              </>
            )}
            <button
              className="nc-meta"
              data-unset={!currentHarness || undefined}
              onClick={() => openBoxPicker('harness')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'harness'}
              data-picker="harness"
              title={hintHarness('Harness')}
            >
              <span className="nc-provider" key={currentHarness?.id ?? 'none'}>
                <HarnessMark harness={currentHarness?.id} size={16} />
                {currentHarness?.name ?? 'No harness installed'}
              </span>
            </button>
            {(picker?.kind === 'harness' || harnessLeaving) && (
              <PickerMenu
                label="Harness"
                searchPlaceholder="Search harnesses"
                leaving={harnessLeaving}
                items={available.map((h) => ({ id: h.id, name: h.name }))}
                renderIcon={(h) => <HarnessMark harness={h.id} size={16} />}
                groups={null}
                selectedId={currentHarness?.id ?? ''}
                onChoose={(id) => {
                  setHarness(id)
                  // Effort words belong to the harness's own catalog - a
                  // switch resets the choice to the new harness's default.
                  setEffort(null)
                  setPermission(null)
                  leaveHarness('close')
                }}
                onClose={() => leaveHarness('close')}
                anchor={{ left: picker.x, top: picker.y }}
                footer={
                  // The way in to the harness manager - the only row in
                  // the empty state, and the honest answer when the list
                  // is missing something. It manages this computer's
                  // harnesses; a phone has none (a machine's are in
                  // Machines).
                  isMobile ? null : <button
                    type="button"
                    onClick={() => leaveHarness('manage')}
                  >
                    Manage harnesses…
                  </button>
                }
              />
            )}
            {model && effortTiers.length > 0 && (
              <div className="nc-effort-wrap">
                <button
                  type="button"
                  className="nc-meta"
                  onClick={toggleEffort}
                  aria-haspopup="dialog"
                  aria-expanded={picker?.kind === 'effort'}
                  data-picker="effort"
                  title={hintEffort("The harness's own effort tier")}
                >
                  <FadingLabel text={effortLabel(chosenEffort)} />
                </button>
                {(picker?.kind === 'effort' || effortLeaving) && (
                  <EffortSlider
                    tiers={effortTiers}
                    value={chosenEffort}
                    leaving={effortLeaving}
                    spot={spotAtBox(boxRef.current?.getBoundingClientRect(), { gap: 8 })}
                    fast={fast}
                    fastSupported={currentHarness?.fast_supported === true}
                    onEffort={setEffort}
                    onFast={() => setFast((v) => !v)}
                  />
                )}
              </div>
            )}

          </div>
          <div className="nc-send-group">
            {/* What the agent may do without asking sits by the send button:
                the last thing to check before a message goes. */}
            {permissionWords.length > 0 && (
              <button
                type="button"
                className="nc-meta"
                data-unset={!chosenPermission || undefined}
                onClick={openPermissionPicker}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'permission'}
                data-picker="permission"
                title={hintPermission('What the harness may do without asking')}
              >
                {permissionWords.find((w) => (w.w ?? w) === chosenPermission)?.l ?? chosenPermission ?? 'Permission'}
              </button>
            )}
            {(picker?.kind === 'permission' || permissionLeaving) && (
              <PickerMenu
                label="Permission"
                hideSearch
                leaving={permissionLeaving}
                items={permissionWords.map((word) => ({ id: word.w ?? word, name: word.l ?? word.w ?? word }))}
                groups={null}
                selectedId={chosenPermission ?? ''}
                onChoose={(id) => {
                  setPermission(id)
                  leavePermission()
                }}
                onClose={leavePermission}
                anchor={{ left: picker.x, top: picker.y }}
                renderSubline={(item) =>
                  item.id === currentHarness?.default_permission ? (
                    <span className="nc-item-sub">
                      {harnessName(currentHarness?.id)} runs this by default
                    </span>
                  ) : null
                }
              />
            )}
            <button className="nc-send" onClick={start} disabled={!canSend} title="Start task">
              <ArrowUp size={16} weight="bold" />
            </button>
          </div>
        </div>
      </div>
      <div className="nc-under">
        {/* The picked harness isn't signed in: one quiet row under the
            chatbox - its mark, the fact, and the way in. Not a card. */}
        {usingApiKey && (
          <div className="nc-auth nc-auth--key">
            <p className="nc-auth-note">Using API Key</p>
            {SIGNIN_HARNESSES.has(currentHarness.id) && (
              <button type="button" className="nc-auth-btn nc-auth-btn--quiet" onClick={() => setSignInFor(currentHarness.id)}>
                Sign in
              </button>
            )}
          </div>
        )}
        {needsSignIn && (
          <div className="nc-auth">
            <HarnessMark harness={currentHarness.id} size={16} />
            <p className="nc-auth-note">
              <span className="nc-auth-name">{harnessName(currentHarness.id)}</span>
              {SIGNIN_HARNESSES.has(currentHarness.id) ? (
                <> isn’t signed in on {targetItem?.name ?? 'this computer'}.</>
              ) : (
                <> isn’t signed in - it signs in through its own setup.</>
              )}
            </p>
            {SIGNIN_HARNESSES.has(currentHarness.id) && (
              <button type="button" className="nc-auth-btn" onClick={() => setSignInFor(currentHarness.id)}>
                Sign in <ArrowRight size={13} />
              </button>
            )}
          </div>
        )}
        <div className="nc-below">
          {/* The project is picked, never typed: the registered projects
              on the machine the chat runs on, "No project" for a chat
              that belongs to none, and the one way to add - the picker's
              own Add project row opens the folder picker. */}
          <div className="nc-device">
            <button
              className="nc-meta"
              onClick={openProjectPicker}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'project'}
              title={chosenProject ? (privacy ? 'The project this chat runs in' : workspace) : 'A chat can run in a project, or in no folder at all'}
            >
              <FolderOpen size={15} weight="duotone" />
              {chosenProject ? <span className="pv">{projectName(chosenProject)}</span> : 'No project'}
            </button>
            {picker?.kind === 'project' && (
              <PickerMenu
                label="Project"
                searchPlaceholder="Search projects"
                wide
                items={[
                  { id: '__none__', name: 'No project' },
                  ...targetProjects.map((p) => ({ id: p.id, name: projectName(p), veiled: true })),
                ]}
                groups={null}
                selectedId={chosenProject?.id ?? '__none__'}
                // The list under this picker is the last one the daemon sent: a failed
                // fetch keeps it rather than showing none, so a machine mid-reconnect no
                // longer reads as a machine with no projects.
                note={
                  projectsFailed
                    ? 'The project list did not load. This is the last one this window saw.'
                    : null
                }
                onChoose={(id) => {
                  setProjectId(id === '__none__' ? null : id)
                  setPicker(null)
                }}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderSubline={(item) =>
                  item.id === '__none__' ? null : (
                    <span className="nc-item-sub pv">
                      {targetProjects.find((p) => p.id === item.id)?.folders[0]}
                    </span>
                  )
                }
                footer={
                  <button
                    type="button"
                    onClick={() => {
                      setPicker(null)
                      setPicking(true)
                    }}
                  >
                    <Plus size={12} weight="bold" /> Add project…
                  </button>
                }
              />
            )}
          </div>
          {cloudMode ? (
            <div className="nc-device">
              <button
                className="nc-meta"
                data-unset={!targetItem || undefined}
                onClick={openPicker('machine')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'machine'}
                title="Where the chat runs"
              >
                <span className={`nc-dev-dot nc-dev-dot--${targetOffline ? 'offline' : 'online'}`} />
                {targetItem ? <span className={targetItem.here ? undefined : 'pv'}>{targetItem.name}</span> : isMobile ? 'Pick a machine' : 'This computer'}
              </button>
              {picker?.kind === 'machine' && (
                <PickerMenu
                  label="Run on"
                  searchPlaceholder="Search machines"
                  items={machineOptions.map((m) => ({ ...m, id: m.id ?? 'here', veiled: !m.here }))}
                  groups={null}
                  selectedId={target ?? 'here'}
                  onChoose={(id) => chooseTarget(id === 'here' ? null : id)}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderIcon={(m) =>
                    m.here ? (
                      <House size={14} weight="duotone" />
                    ) : (
                      <span className={`nc-dev-dot nc-dev-dot--${m.online ? 'online' : 'offline'}`} />
                    )
                  }
                  renderSubline={(m) =>
                    m.here ? null : (
                      <span className="nc-item-sub">{m.online ? platformLabel(m.platform) : 'Offline'}</span>
                    )
                  }
                />
              )}
            </div>
          ) : null}
        </div>
        {addProjectError && <p className="nc-error">{addProjectError}</p>}
        {targetOffline && <p className="nc-error"><span className="pv">{targetItem.name}</span> is offline.</p>}
        {isMobile && cloudMode && machineOptions.length === 0 && (
          <div className="nc-auth nc-auth--solo">
            <p className="nc-auth-note">
              Chats run on your machines - add one and start chats on it from here.
            </p>
            <button type="button" className="nc-auth-btn" onClick={() => onOpen('machines')}>
              Machines <ArrowRight size={13} />
            </button>
          </div>
        )}
        {picking && (
          <FolderPickerModal
            api={apiFor(target)}
            where={targetItem?.name ?? 'this computer'}
            start={workspace}
            onChoose={chooseProjectFolder}
            onClose={() => {
              setPicking(false)
              onAddProjectDone?.()
            }}
          />
        )}
        {connection.state === 'error' && <p className="nc-error">{connection.error}</p>}
        {error && <p className="nc-error">{error}</p>}
        {signInFor && (
          <SignInModal
            harnessId={signInFor}
            onDone={() => setSignInFor(null)}
          />
        )}
        {managingHarnesses && (
          <HarnessManagerModal
            onClose={() => setManagingHarnesses(false)}
            onSignIn={(id) => {
              setManagingHarnesses(false)
              setSignInFor(id)
            }}
          />
        )}
        {manageModels && currentHarness && (
          <EndpointManagerModal
            harness={currentHarness.id}
            deviceId={target}
            where={targetItem?.name ?? 'this computer'}
            onClose={() => setManageModels(false)}
            onChanged={reloadEndpoints}
          />
        )}

      </div>
    </div>
  )
}
