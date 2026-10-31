// The rules list's search, wired: one field, one filtered list, the count
// line only while filtering, and the two empty answers kept apart. The
// matching itself lives in automation.js and is pinned in automation.test.js.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = (file) => readFileSync(new URL(`./${file}`, import.meta.url), 'utf8')

test('one search field over the list, and the field and its hint arrive together', () => {
  const view = source('AutomationsView.jsx')
  assert.match(view, /import \{[\s\S]{0,220}ruleSearchHint,/)
  assert.match(view, /import \{[\s\S]{0,220}searchRules,/)
  // The list, the query, and the daemon's harness list the labels come from:
  // all three feed the search, so a plugin's harness name is matched too.
  assert.match(
    view,
    /const shown = useMemo\(\(\) => searchRules\(rules, query, harnesses\), \[rules, query, harnesses\]\)/,
  )
  // The threshold is the rule's: below it there is no field, so the box can
  // never be an icon with no words in it.
  assert.match(view, /\{rules\.length >= RULE_SEARCH_HINT_FROM && \(\s*<div className="am-toolbar">/)
  assert.match(view, /import \{[\s\S]{0,220}RULE_SEARCH_HINT_FROM,/)
  assert.match(view, /className="am-search-input"/)
  assert.match(view, /aria-label="Search rules"/)
  // The hint is the rule's, not a hardcoded string.
  assert.match(view, /placeholder=\{ruleSearchHint\(rules\.length\)\}/)
  assert.doesNotMatch(view, /placeholder="Search rules"/)
})

test('Escape clears the field, then leaves it, like the sidebar search', () => {
  const view = source('AutomationsView.jsx')
  assert.match(view, /if \(e\.key !== 'Escape'\) return/)
  assert.match(view, /if \(query\) setQuery\(''\)/)
  assert.match(view, /else e\.currentTarget\.blur\(\)/)
  // And the field has its own clear button while there is something to clear.
  assert.match(view, /className="am-search-clear"/)
})

test('the list draws what the query matched, in the order it already had', () => {
  const view = source('AutomationsView.jsx')
  assert.match(view, /\{shown\.map\(\(rule\) => \{/)
  assert.doesNotMatch(view, /\{rules\.map\(\(rule\) => \{/)
  // The header still counts every rule: the search narrows the list, not the
  // inventory.
  assert.match(view, /rules\.length === 1 \? 'rule' : 'rules'\} · \$\{enabledCount\} enabled/)
})

test('the count line appears while filtering, and the empty answers stay apart', () => {
  const view = source('AutomationsView.jsx')
  assert.match(view, /const filtering = query\.trim\(\)\.length > 0/)
  assert.match(view, /filtering && shown\.length > 0 && \(/)
  assert.match(view, /ruleCountLine\(shown\.length, rules\.length\)/)
  // No rules at all: the invitation, only once the daemon has answered.
  assert.match(view, /\{automationsAnswered \? RULE_EMPTY_NONE : 'Loading the rules…'\}/)
  // A query that found nothing: its own sentence, and never the invitation.
  assert.match(view, /filtering && rules\.length > 0 && shown\.length === 0/)
  assert.match(view, /ruleEmptyLine\(query\)/)
})

test('one noun and one verb: the door, the empty list and the form agree', () => {
  const view = source('AutomationsView.jsx')
  // The door says "New rule", the console's own "New <thing>" shape.
  assert.match(view, />\s*New rule\s*</)
  assert.doesNotMatch(view, /New automation/)
  // The empty list borrows the verb the form's button uses, and the noun both
  // of them use is the contract's.
  assert.match(source('automation.js'), /RULE_EMPTY_NONE = 'No rules yet\. Create one\.'/)
  assert.match(source('AutomationForm.jsx'), /'Save rule' : 'Create rule'/)
  // The draft card and the search speak it too.
  assert.match(view, /CREATE RULE/)
  assert.match(view, /aria-label="Search rules"/)
})

test('the field and the count have styles of their own, motion covered twice', () => {
  const css = source('automations.css')
  for (const rule of ['.am-toolbar', '.am-search', '.am-search-input', '.am-search-clear', '.am-count']) {
    assert.match(css, new RegExp(rule.replace('.', '\\.')), `${rule} is styled`)
  }
  // The field fades its border on focus like every other field; the two
  // motion switches cover it.
  assert.match(css, /@media \(prefers-reduced-motion: reduce\) \{\s*\.am-search-input \{/)
  assert.match(css, /html\[data-reduce-motion\] \.am-search-input \{/)
})

test('the no-machine line sits above the list and never gates it', () => {
  const view = source('AutomationsView.jsx')
  const css = source('automations.css')
  // The rule is automation.js's; the view only renders what it says. The
  // import is part of that: a call site without one is a ReferenceError that
  // the source tests would otherwise never see, and it crashed this view
  // once. Every module the call needs is named here.
  assert.match(view, /import \{[\s\S]{0,400}machineNotice,/)
  assert.match(view, /const machineRow = machineNotice\(\{/)
  assert.match(view, /answered: machinesAnswered,/)
  assert.match(view, /signedIn: signedIn\(cloud\),/)
  // It is above the list, and the list itself is not conditional on it.
  const machineAt = view.indexOf('className="am-machine"')
  const listAt = view.indexOf('className="am-list"')
  assert.ok(machineAt > 0 && listAt > machineAt, 'the line is above the rules')
  assert.doesNotMatch(view, /\{machineRow && \(\s*<div className="am-list"/)
  assert.match(view, /\{shown\.map\(\(rule\) => \{/)
  // One action, pointing at the control that changes it - the same view ids
  // the palette and the shell already navigate by.
  assert.match(view, /kb\.run\(machineRow\.action === 'account' \? 'view-account' : 'view-machines'\)/)
  // Its own quiet row, in the list's own column.
  for (const rule of ['.am-machine', '.am-machine-text', '.am-machine-act']) {
    assert.match(css, new RegExp(rule.replace('.', '\\.')), `${rule} is styled`)
  }
})