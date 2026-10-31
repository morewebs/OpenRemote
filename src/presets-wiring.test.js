// The preset chips are a browser surface: their behaviour is a row of
// buttons inside NewChat.jsx, which this suite cannot render (there is no
// test renderer in this repo). So the wiring is pinned the way the other
// wiring tests here pin it - the expression that makes the behaviour
// possible has to be present, and the rules it calls are unit tested in
// presets.test.js. The one CSS rule is pinned too, because the ghost look
// is a design decision, not a preference: a filled pill row would be a
// regression.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')
const newchat = source('NewChat.jsx')
const css = source('newchat.css')

test('the chips row sits at the top of the page, as real buttons', () => {
  // It moved out of the composer box: the row is its own value now, rendered in the
  // page's first grid row beside the floating bar, because in the box it crowded
  // the pickers under it.
  const head = newchat.indexOf('className="nc-page-head"')
  const box = newchat.indexOf('className={`nc-box')
  const textarea = newchat.indexOf('placeholder="Describe the task"')
  assert.ok(newchat.includes('const presetsRow = ('), 'the row is its own value')
  assert.ok(head > 0 && box > head, 'the head renders before the box')
  assert.ok(textarea > box, 'and the box still holds the textarea')
  const headBlock = newchat.slice(head, box)
  assert.match(headBlock, /\{presetsRow\}/, 'the head renders it')
  assert.doesNotMatch(headBlock, /className="nc-presets"/, 'and the box does not')
  assert.match(css, /\.nc-page-head \{[^}]*position: absolute/, 'the head is out of the grid, so it cannot displace the greeting')
  assert.match(css, /\.nc-page-head \{[^}]*pointer-events: none/, 'and its empty parts let a click through')
  assert.match(css, /\.nc-page-head > \* \{[^}]*pointer-events: auto/, 'while the chips still take theirs')
  assert.match(newchat, /role="group" aria-label="Saved setups"/)
  assert.match(newchat, /<button[\s\S]{0,120}className={`nc-preset/)
})

test('the Save chip names the setup and is renamed in place, one step', () => {
  assert.match(newchat, />\s*Save this setup\s*</)
  assert.match(newchat, /setPresetEditing\(\{ id: 'new', draft: suggestedForNow\(\) \}\)/)
  assert.match(newchat, /aria-label="Name this setup"/)
  // Enter keeps the name, Escape drops it, and the blur Enter causes cannot
  // save the same setup a second time.
  assert.match(newchat, /if \(e\.key === 'Enter'\) \{[\s\S]{0,120}commitPresetName\(\)/)
  assert.match(newchat, /if \(e\.key === 'Escape'\) \{[\s\S]{0,120}cancelPresetName\(\)/)
  assert.match(newchat, /presetEditingRef\.current = null/)
})

test('a saved preset is written through storePresets, never in memory only', () => {
  assert.match(newchat, /presets, setPresets\] = useState\(\(\) => loadPresets\(\)\)/)
  assert.match(newchat, /setPresets\(storePresets\(\[\.\.\.presets, preset\]\)\)/)
  assert.match(newchat, /setPresets\(storePresets\(replacePreset\(/)
  assert.match(newchat, /setPresets\(storePresets\(removePreset\(/)
  assert.match(newchat, /setPresets\(storePresets\(markDefault\(/)
})

test('one preset is the default, and New chat opens on it', () => {
  assert.match(newchat, /defaultPreset\(loadPresets\(\)\)/)
  assert.match(newchat, /useState\(\(\) => openingPreset\?\.harness \?\? loadDefaults\(\)\.harness\)/)
  assert.match(newchat, /const pendingPreset = useRef\(openingPreset \?\? null\)/)
  assert.match(newchat, /aria-pressed=\{preset\.default === true\}/)
})

test('deleting is a hover control that asks once, through row-confirm', () => {
  assert.match(newchat, /import \{ armRow, clearRow, rowIsArmed \} from '\.\/row-confirm\.js'/)
  assert.match(newchat, /rowIsArmed\(armedPreset, preset\.id\)/)
  assert.match(newchat, /armed \? deletePreset\(preset\.id\) : setArmedPreset\(\(curr\) => armRow\(curr, preset\.id\)\)/)
  assert.match(newchat, /\{armed \? 'Delete\?' : <X size=\{11\}/)
})

test('a preset that cannot be honoured applies what it can, and says nothing about it', () => {
  // The founder's call: a saved setup whose model or project is gone settles to what is
  // available here without narrating the comparison. What it can still carry, it carries.
  assert.match(newchat, /settlePreset\(preset, \{/)
  assert.match(newchat, /if \(slots\.projectId\) setProjectId\(slots\.projectId\)/)
  assert.doesNotMatch(newchat, /setPresetNote/, 'no note is set any more')
  assert.match(newchat, /\{\(carryNote \|\| selectionNote\) && \(/, 'the chat-level notes stay')
})

test('a preset is a chip with an edge at rest, and the mark that says its harness', () => {
  // It used to be bare grey text at caption size, which read as leftover prose
  // rather than as something you can press. The edge and the wash live on the
  // wrapper, because the wrapper is the chip; the button inside carries the name.
  const wrap = css.slice(css.indexOf('.nc-preset-wrap {'), css.indexOf('.nc-preset {'))
  assert.match(wrap, /border: 1px solid var\(--line\)/)
  assert.match(wrap, /border-radius: 999px/)
  assert.match(wrap, /background: rgba\(255, 255, 255, 0\.028\)/)
  const chip = css.slice(css.indexOf('.nc-preset {'), css.indexOf('.nc-preset:hover'))
  assert.match(chip, /display: inline-flex/)
  assert.match(chip, /gap: 7px/)
  assert.doesNotMatch(chip, /background: rgba/, 'the wash belongs to the wrapper, not the name')
  assert.match(newchat, /<HarnessMark harness=\{preset\.harness\} size=\{12\} \/>/)
})

// ---- saving a chat's setup as a preset ----
//
// The entry point is a palette row on the chat on screen; CommandPalette.jsx is
// held by the rename lift tonight, so the row is described in the report rather
// than pinned here. What is pinned is the surface the row opens and the rules
// it saves through.

const modal = source('PresetNameModal.jsx')

test('the name prompt is a member of the modal family, with the family ways out', () => {
  assert.match(modal, /className="dv-modal-backdrop"/)
  assert.match(modal, /className="dv-modal"/)
  assert.match(modal, /className="dv-modal-head"/)
  assert.match(modal, /className="dv-modal-title"/)
  assert.match(modal, /className="dv-modal-close"/)
  assert.match(modal, /role="dialog"/)
  assert.match(modal, /aria-modal="true"/)
  assert.match(modal, /e\.target === e\.currentTarget/)
  assert.match(modal, /onClick=\{onClose\}/)
  assert.match(modal, /useDialog\(panelRef, onClose\)/)
  assert.match(modal, /useBack\(true, onClose\)/)
})

test('it asks for a name the way the rename dialog does', () => {
  // The same field, the same rules, the suggested name prefilled and selected.
  assert.match(modal, /import \{ MAX_NAME, renameState \} from '\.\/rename-chat\.js'/)
  assert.match(modal, /const verdict = renameState\(null, text\)/)
  assert.match(modal, /useState\(preset\?\.name \?\? ''\)/)
  assert.match(modal, /input\.focus\(\)\s*input\.select\(\)/)
  assert.match(modal, /if \(e\.key !== 'Enter'\) return/)
  assert.match(modal, /disabled=\{busy \|\| verdict\.state === 'empty'\}/)
  // The field cannot take a name the chip could not show.
  assert.match(modal, /maxLength=\{Math\.min\(NAME_LIMIT, MAX_NAME\)\}/)
  // And it says what is being saved, in the chip's own words.
  assert.match(modal, /presetSummary\(preset\)/)
})

test('a refused save keeps the dialog open with its sentence', () => {
  assert.match(modal, /if \(result\?\.error\) \{\s*setError\(result\.error\)/)
  assert.match(modal, /\{error && <p className="cv-error-line">\{error\}<\/p>\}/)
  // Only an accepted save closes it.
  assert.match(modal, /onClose\(\)\s*\}\s*catch/)
})
