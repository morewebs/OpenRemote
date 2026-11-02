// Presets for New chat: a named setup - harness, model, effort, permission
// word, project, fast - that the person saves once and picks again from a
// row of chips. Console-side and local: a preset is a preference, so it
// lives in this computer's storage rather than in the daemon, and it can
// never constrain a chat, only preselect it.
//
// A preset is a record of what was true when it was saved, and models and
// effort tiers churn with harness updates. So every read repairs what it
// can (settings.js holds the tolerant storage, this file holds the rules),
// and settling a preset against the lists a harness offers today returns
// the slots that are still true plus one calm sentence per slot that is
// not - a preset that cannot be honoured in full still applies what it can.

import { loadStoredPresets, saveStoredPresets } from './settings.js'
import { harnessName } from './harness-names.js'
import { prettyModelName } from './model-display.js'
import { projectName } from './projects.js'

// Enough chips for a person to keep their few ways of working, few enough
// that the row stays a row.
export const PRESET_LIMIT = 12
// A chip has to stay one line: past this a name is a paragraph.
export const NAME_LIMIT = 60

let idCounter = 0

/** A stable id for a new preset (not a storage key: only React and deletes use it). */
export function newPresetId() {
  idCounter += 1
  const rand = globalThis.crypto?.randomUUID?.()
  return rand ? `p-${rand.slice(0, 8)}` : `p-${Date.now().toString(36)}-${idCounter}`
}

function text(value) {
  return typeof value === 'string' && value.trim() ? value.trim() : null
}

function bool(value) {
  return value === true
}

/**
 * One stored row, repaired. Returns null for a row that cannot be a preset
 * at all (no object, no harness): a preset without a harness names nothing
 * a chat could start on, so it is dropped rather than shown broken.
 */
export function normalizePreset(raw) {
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const harness = text(raw.harness)
  if (!harness) return null
  const name = text(raw.name)
  return {
    id: text(raw.id) ?? newPresetId(),
    // A missing name is not damage: it is named after what it is when the
    // row is first shown, which is the same words saving would have used.
    name: name ? name.slice(0, NAME_LIMIT) : null,
    harness,
    model: text(raw.model),
    effort: text(raw.effort),
    permission: text(raw.permission),
    projectId: text(raw.projectId),
    fast: bool(raw.fast),
    default: bool(raw.default),
  }
}

/**
 * A stored list, repaired: junk dropped, ids made unique, the cap applied,
 * and at most one row still marked as the default (the first one, since a
 * second default is a damaged write rather than a second opinion).
 */
export function normalizePresets(rows) {
  const list = Array.isArray(rows) ? rows : []
  const out = []
  const seen = new Set()
  let defaulted = false
  for (const raw of list) {
    if (out.length >= PRESET_LIMIT) break
    const preset = normalizePreset(raw)
    if (!preset) continue
    if (seen.has(preset.id)) preset.id = newPresetId()
    seen.add(preset.id)
    if (preset.default && !defaulted) defaulted = true
    else preset.default = false
    out.push(preset)
  }
  return out
}

/** The presets this computer has, repaired. */
export function loadPresets() {
  return normalizePresets(loadStoredPresets())
}

/** Store the list as repaired, and answer the list that was stored. */
export function storePresets(presets) {
  const clean = normalizePresets(presets)
  saveStoredPresets(clean)
  return clean
}

/** The default preset, if one is marked: what New chat opens on. */
export function defaultPreset(presets) {
  return (presets ?? []).find((p) => p.default) ?? null
}

/**
 * What the current setup is, as a preset row. `state` carries the settled
 * values New chat already holds; an empty name is filled in by
 * suggestedPresetName at the moment of saving, so the stored row is never
 * nameless.
 */
export function presetFromState(state) {
  return normalizePreset({
    id: newPresetId(),
    name: state.name ?? null,
    harness: state.harness,
    model: state.model ?? null,
    effort: state.effort ?? null,
    permission: state.permission ?? null,
    projectId: state.projectId ?? null,
    fast: state.fast ?? false,
    default: false,
  })
}

/** Replace one preset by id, leaving the rest (and the default flag) alone. */
export function replacePreset(presets, preset) {
  const clean = normalizePreset(preset)
  if (!clean) return normalizePresets(presets)
  return normalizePresets((presets ?? []).map((p) => (p.id === clean.id ? { ...clean, default: p.default } : p)))
}

/** Drop one preset by id. Marking a default is the caller's job. */
export function removePreset(presets, id) {
  return normalizePresets((presets ?? []).filter((p) => p.id !== id))
}

/** Mark `id` as the only default, or clear the mark when id is null. */
export function markDefault(presets, id) {
  return normalizePresets((presets ?? []).map((p) => ({ ...p, default: p.id === id })))
}

/**
 * A name after what the preset is, in the words the console already shows:
 * "Opus high, this repo". Labels come from the caller so this stays pure;
 * a missing model falls back to the harness's name.
 */
export function suggestedPresetName({ modelLabel = null, harnessLabel = null, effort = null, projectLabel = null } = {}) {
  const what = modelLabel ?? harnessLabel ?? 'Chat'
  const how = effort ? ` ${effort}` : ''
  const where = projectLabel ? `, ${projectLabel}` : ''
  return `${what}${how}${where}`.slice(0, NAME_LIMIT)
}

/** One line for a chip's title: everything the preset asks for. */
export function presetSummary(preset, labels = {}) {
  if (!preset) return ''
  const parts = [labels.harnessLabel ?? preset.harness]
  if (preset.model) parts.push(labels.modelLabel ?? preset.model)
  if (preset.effort) parts.push(`${preset.effort} effort`)
  if (preset.permission) parts.push(labels.permissionLabel ?? preset.permission)
  if (preset.fast) parts.push('fast')
  if (preset.projectId) parts.push(labels.projectLabel ?? 'a project')
  return parts.join(' · ')
}

/**
 * Settle a preset against what the chosen harness offers today.
 *
 * `world` is plain data:
 *   harnesses     the harness ids installed on this computer
 *   models        the chosen harness's model rows ({ model, reasoning_efforts })
 *   permissions   the chosen harness's permission words ([{ w, l }])
 *   projects      the project ids on the machine this chat will run on
 *   fastSupported the harness ids whose fast mode is usable today, when the
 *                 caller has that list: a preset's fast flag rides its own
 *                 harness, and without this list the flag is gated on the
 *                 harness merely being installed
 *   labels        what to call the harness, the model and the project in a sentence
 *
 * Returns the slots to apply and one sentence per slot that could not be
 * honoured. The caller applies the slots; nothing here touches storage or
 * React, so the rules are testable on their own.
 */
export function settlePreset(preset, world = {}) {
  const notes = []
  const harnessOk = preset?.harness && (world.harnesses ?? []).includes(preset.harness)
  const labels = world.labels ?? {}
  const harnessWord = labels.harnessLabel ?? preset?.harness ?? 'that app'
  if (!preset) return { slots: null, notes: [] }
  if (!harnessOk) {
    notes.push(`Its app is not installed on this computer any more, so this chat stays on ${labels.currentHarnessLabel ?? 'the one selected'}.`)
  }

  const models = harnessOk ? (world.models ?? []) : []
  let model = null
  if (preset.model) {
    const row = models.find((m) => m.model === preset.model)
    if (row) model = row.model
    else if (harnessOk) {
      notes.push(`${labels.modelLabel ?? preset.model} is not one ${harnessWord} lists any more, so the chat starts on its default model.`)
    }
  }

  // An effort tier belongs to the model it was picked on: with the model
  // gone, the tier has nothing to sit on, and one note is enough.
  let effort = null
  if (preset.effort) {
    const tiers = model ? (models.find((m) => m.model === model)?.reasoning_efforts ?? []) : []
    if (model && tiers.includes(preset.effort)) effort = preset.effort
    else if (model && harnessOk) {
      notes.push(`${labels.effortLabel ?? preset.effort} effort is not one that model offers any more, so the chat runs at the app's own effort.`)
    }
  }

  let permission = null
  if (preset.permission) {
    const words = harnessOk ? (world.permissions ?? []) : []
    if (words.some((w) => w.w === preset.permission)) permission = preset.permission
    else if (harnessOk) {
      notes.push(`The approval word ${labels.permissionLabel ?? preset.permission} is not one ${harnessWord} offers any more, so the app's own default holds.`)
    }
  }

  let projectId = null
  if (preset.projectId) {
    if ((world.projects ?? []).includes(preset.projectId)) projectId = preset.projectId
    else notes.push('Its project is not on this machine any more, so the chat starts without a workspace.')
  }

  // Fast belongs to the harness it was saved on: with that harness gone the
  // flag would land on whichever harness is selected, which may have no fast
  // mode at all. So it rides the harness, and where the caller says which
  // harnesses still have a fast mode, it rides that list too. Nothing is said
  // about it: the harness sentence above already explains the substitution,
  // and a second note for a checkbox would be noise.
  const fastOk =
    harnessOk &&
    preset.fast === true &&
    (!world.fastSupported || (world.fastSupported ?? []).includes(preset.harness))

  return {
    slots: {
      harness: harnessOk ? preset.harness : null,
      model,
      effort,
      permission,
      projectId,
      fast: fastOk,
    },
    notes,
  }
}

// ---- saving a chat's setup as a preset ----
//
// A chat knows the same six choices a preset holds, and the person who just
// ran one should not have to rebuild it in New chat. The carry rule is the one
// above read backwards: what a preset can honestly hold is what the chat can
// honestly carry, and a field the chat cannot supply is left unset rather than
// guessed.
//
//   harness   the chat's harness, when it is installed on this computer
//   model     the model the chat ran, in the spelling the chat reports it in:
//             a preset is a record of what was true, and settlePreset repairs
//             a model the harness no longer lists when it is applied
//   effort    the chat's effort, and only alongside a model - an effort tier
//             belongs to the model it was picked on
//   permission the chat's own approval word, only while the harness still
//             offers it (otherwise the harness default is the honest answer)
//   project   the registered project whose folder the chat ran in, and only
//             when the chat ran here: a project on another computer is not a
//             project on this one
//   fast      only where the harness has a fast mode at all
//
// A chat whose harness is not installed here cannot become a preset either:
// the preset would name a harness no chat on this computer could start on.

/** Why a chat cannot become a preset, in one plain sentence each. */
export const PRESET_REFUSALS = {
  'no-chat': 'No chat is on screen to take a setup from.',
  'no-harness': 'That chat has no app, so there is no setup to save.',
  'not-installed': 'Its app is not installed on this computer, so a setup could not start a chat here.',
  full: `This computer already keeps ${PRESET_LIMIT} setups; delete one to save another.`,
}

/**
 * What a chat yields for a preset: `{ offer, reason, preset, suggestedName,
 * carried, leftOut }`. `preset` is a stored row with the suggested name
 * already in it; the dialog lets the person change that name before it is
 * written. `carried` names the fields the chat supplied, `leftOut` the ones it
 * could not, so a caller never has to read the chat's fields by hand.
 *
 * `world` is plain data:
 *   harnesses   this computer's capability rows ({ id, name, available,
 *               permission_modes, fast_supported })
 *   projects    the registered projects on this computer ({ id, folders })
 *   thisDevice  this computer's device id, for "did the chat run here"
 *   labels      optional display labels for the suggested name
 */
export function presetFromChat(chat, world = {}) {
  if (!chat) return refusedPlan('no-chat')
  const harnessId = text(chat.harness)
  if (!harnessId) return refusedPlan('no-harness')
  const row = (world.harnesses ?? []).find((h) => h && h.id === harnessId) ?? null
  if (!row || row.available !== true) return refusedPlan('not-installed')

  const model = text(chat.model)
  // No model, no effort: an effort tier only means something on a model.
  const effort = model ? text(chat.effort) : null
  const word = text(chat.permissionMode)
  const permission = word && (row.permission_modes ?? []).includes(word) ? word : null
  const ranHere = !chat.executor || chat.executor === world.thisDevice
  const workspace = text(chat.workspace)
  const project =
    ranHere && workspace
      ? (world.projects ?? []).find((p) => (p.folders ?? []).includes(workspace)) ?? null
      : null
  const fast = row.fast_supported === true && chat.fast === true

  const labels = world.labels ?? {}
  const suggestedName = suggestedPresetName({
    modelLabel: labels.modelLabel ?? (model ? prettyModelName(model) : null),
    harnessLabel: labels.harnessLabel ?? row.name ?? harnessName(harnessId),
    effort,
    projectLabel: labels.projectLabel ?? (project ? projectName(project) : null),
  })
  const preset = normalizePreset({
    id: newPresetId(),
    name: suggestedName,
    harness: harnessId,
    model,
    effort,
    permission,
    projectId: project?.id ?? null,
    fast,
    default: false,
  })
  // Which of the six the preset ends up holding, and which it does without:
  // the caller can say what was carried without inspecting the chat itself.
  // `fast` is carried whenever the harness has a fast mode at all, because the
  // chat's boolean is then a real answer (off is an answer); a harness with no
  // fast mode leaves the slot out.
  const carried = ['harness', 'model', 'effort', 'permission', 'project'].filter((field) =>
    Boolean(preset?.[field === 'project' ? 'projectId' : field]),
  )
  if (row.fast_supported === true) carried.push('fast')
  const leftOut = ['harness', 'model', 'effort', 'permission', 'project', 'fast'].filter(
    (field) => !carried.includes(field),
  )
  return { offer: true, reason: null, preset, suggestedName, carried, leftOut }
}

function refusedPlan(reason) {
  return { offer: false, reason, preset: null, suggestedName: null, carried: [], leftOut: [] }
}

// Whoever is showing the chips can hear about a save the moment it lands, so
// a setup saved from a chat appears without reloading the app. New chat reads
// the stored list when it mounts; this is for a row that is already on screen.
const listeners = new Set()

/** Follow saved-list changes. Returns the undo. */
export function onPresetsChanged(fn) {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

/**
 * Append one preset to this computer's list, through the same storage every
 * preset goes through. Returns `{ preset, presets, error }`: an `error` is one
 * of PRESET_REFUSALS' sentences and means nothing was written.
 *
 * The name prompt hands its row here, so the name a person typed is the one
 * stored (repaired to the chip's limit). The cap is applied by the repair
 * rules on write, which keep the first rows and drop the rest: a thirteenth
 * save would vanish silently, so it is refused here instead, with the number
 * said.
 */
export function savePreset(preset) {
  const clean = normalizePreset(preset)
  if (!clean) {
    return { preset: null, presets: loadPresets(), error: PRESET_REFUSALS['no-harness'] }
  }
  const existing = loadPresets()
  if (existing.length >= PRESET_LIMIT) {
    return { preset: null, presets: existing, error: PRESET_REFUSALS.full }
  }
  const presets = storePresets([...existing, clean])
  for (const fn of listeners) fn(presets)
  return { preset: clean, presets, error: null }
}

/**
 * Save a chat's setup as a new preset: the carry rule, then the storage. A
 * chat that cannot become a preset is refused with its own sentence.
 */
export function saveChatPreset(chat, world = {}) {
  const plan = presetFromChat(chat, world)
  if (!plan.offer) {
    return {
      preset: null,
      presets: null,
      error: PRESET_REFUSALS[plan.reason] ?? 'That setup could not be saved.',
    }
  }
  return savePreset(plan.preset)
}
