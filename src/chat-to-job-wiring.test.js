// Turning a chat into a recurring job, wired: the palette row that offers it
// for the chat on screen, the two lines that carry its draft to the rule
// editor's view, and the editor's own part - a prefilled form that is not
// dirty until a person touches it.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('the palette offers the command for the chat on screen, when a chat can seed one', () => {
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /import \{ planJob, requestJob \} from '\.\/chat-to-job\.js'/)
  // The same chat the Changes row reads, and the same three lists a rule is
  // built from.
  assert.match(palette, /planJob\(reviewChat, \{/)
  assert.match(palette, /harnesses: capabilities\?\.harnesses \?\? \[\]/)
  assert.match(palette, /machines: machines \?\? \[\]/)
  assert.match(palette, /projects: projects \?\? \[\]/)
  // Offered only when the plan says a chat can seed one.
  assert.match(palette, /const jobRow = job\.offer/)
  assert.match(palette, /name: 'Turn this chat into a recurring job'/)
  assert.match(palette, /run: \{ job: job\.draft \}/)
})

test('choosing it stages the draft and asks the shell for the Automations view', () => {
  const palette = source('CommandPalette.jsx')
  // The stage has to happen before the navigation: the view picks it up as it
  // mounts, and the palette has no prop that could carry it there.
  assert.match(palette, /else if \(r\.job\) \{/)
  assert.match(palette, /requestJob\(r\.job\)\s*kb\.run\('view-automations'\)/)
})

test('the rows the palette already had are still offered', () => {
  // Other lanes added rows to this list tonight; this one goes beside them,
  // not instead of them. Membership is pinned rather than order, so a row
  // another lane inserts between them does not break this case.
  const palette = source('CommandPalette.jsx')
  assert.match(palette, /key: 'action:changes'/, 'the changes review row')
  assert.match(palette, /key: 'action:stop-all'/, 'the stop everything row')
  const list = palette.slice(palette.indexOf('actions: ['), palette.indexOf('chats: chatRows'))
  for (const group of ['stopRow', 'review', 'jobRow', 'pages', 'actions']) {
    assert.match(list, new RegExp(`\\.\\.\\.${group}\\b`), `${group} is still spread into the list`)
  }
  // The stop lane's own pin on this call (stopChat last) is kept.
  assert.match(palette, /stopChat \} = useConsole\(\)/)
})

test('the Automations view collects a staged draft, or hears one while it is open', () => {
  const view = source('AutomationsView.jsx')
  assert.match(view, /import \{ onJobRequest, takeJobRequest \} from '\.\/chat-to-job\.js'/)
  assert.match(view, /const waiting = takeJobRequest\(\)/)
  assert.match(view, /if \(waiting\) open\(waiting\)/)
  assert.match(view, /return onJobRequest\(open\)/)
  // The draft opens the existing rule editor; nothing saves on the way in.
  assert.match(view, /const open = \(draft\) => setEditing\(draft\)/)
})

test('the editor knows it is drafting from a chat, and leaves the schedule alone', () => {
  const form = source('AutomationForm.jsx')
  assert.match(form, /const fromChat = rule\?\.fromChat === true/)
  // A chat cannot know a schedule or a folder it never had: both open empty,
  // where a blank new rule keeps the form's own 09:00 and first project.
  assert.match(form, /useState\(rule\?\.trigger\?\.time \?\? \(fromChat \? '' : '09:00'\)\)/)
  assert.match(form, /rule\?\.workspace \?\? \(fromChat \? '' : defaultWorkspace\(projects\)\)/)
  // The time field says what it is missing instead of drawing 09:00.
  assert.match(form, /Pick a time/)
  assert.match(form, /className="af-time-empty"/)
})

test('the source line is one line, and it rewrites itself as the person works', () => {
  const form = source('AutomationForm.jsx')
  assert.match(form, /import \{ jobSourceLine \} from '\.\/chat-to-job\.js'/)
  assert.match(form, /\{fromChat && \(/)
  // Live inputs: the trigger kind, whether a time is picked, whether a
  // project is still to choose, and whether the task was shortened.
  assert.match(form, /jobSourceLine\(\{/)
  assert.match(form, /shortened: rule\?\.shortened === true/)
  assert.match(form, /needsProject: !workspace\.trim\(\) && \(projects \?\? \[\]\)\.length > 0/)
  assert.match(form, /when: kind,/)
  assert.match(form, /timed: Boolean\(time\.trim\(\)\)/)
})

test('an untouched prefilled form is not dirty: nothing clears a rule value on mount', () => {
  // The dirty guard compares every field against the snapshot taken on the
  // first render. The model and the permission word are part of that
  // snapshot, so the effect that settles them must run only on a harness the
  // person switched to, or the form would open dirty with nothing touched.
  // What a switch does has moved: it no longer blanks both fields (a rule then
  // saved silently on the harness default) but asks harness-carry.js what the
  // new harness can actually carry. It does not narrate the drop: clearing a
  // field is the whole message, and a sentence about it read as an error.
  const form = source('AutomationForm.jsx')
  assert.match(form, /const harnessAtOpen = useRef\(harness\)/)
  assert.match(form, /if \(harness !== harnessAtOpen\.current\) \{/)
  assert.match(form, /carrySetup\(\s*\n?\s*\{ model, permission \},/)
  assert.doesNotMatch(form, /droppedLine/)
  assert.match(form, /restoreFromCatalog\(pendingCarry\.current, list \?\? \[\]\)/)
  assert.match(form, /harnessAtOpen\.current = harness/)
  // And the snapshot is still taken once, over every field.
  assert.match(form, /opened\.current = \{ name, kind, time, harness, model, permission, machineId, workspace, task, enabled \}/)
})
