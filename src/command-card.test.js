import assert from 'node:assert/strict'
import test from 'node:test'
import { commandSummary, isCommandRun, shellInner } from './command-card.js'

test('a command string with an optional description is a command run', () => {
  assert.equal(isCommandRun({ command: 'npm test' }), true)
  assert.equal(
    isCommandRun({ command: 'Remove-Item -Recurse -Force "$env:TEMP\\x"', description: 'Clean the temp path' }),
    true,
  )
  assert.equal(isCommandRun({ command: '' }), false, 'an empty command is nothing to show')
})

test('other tool inputs are not command runs - by shape, never by name', () => {
  assert.equal(isCommandRun(null), false)
  assert.equal(isCommandRun('plain string'), false)
  assert.equal(isCommandRun({ file_path: '/a', old_string: 'x', new_string: 'y' }), false)
  assert.equal(isCommandRun({ query: 'select *' }), false)
  assert.equal(isCommandRun({}), false)
})

test('the command a person ran comes out of the shell line the harness wrapped it in', () => {
  assert.equal(
    shellInner('"C:\\Program Files\\PowerShell\\7\\pwsh.exe" -NoProfile -Command "rg -n \\"x\\" src"'),
    'rg -n "x" src',
  )
  assert.equal(shellInner("powershell.exe -NoLogo -ExecutionPolicy Bypass -Command 'Get-Content est.ps1'"), 'Get-Content est.ps1')
  assert.equal(shellInner("powershell -Command 'Write-Output ''it''s here'''"), "Write-Output 'it's here'")
  assert.equal(shellInner('/bin/bash -lc "git status --short"'), 'git status --short')
  assert.equal(shellInner("bash -lc 'echo hi'"), 'echo hi')
  assert.equal(shellInner('cmd.exe /d /s /c dir'), 'dir')
})

test('a line that is not a readable shell wrapper comes back as it was', () => {
  for (const line of [
    'git status',
    'powershell -Command',
    'powershell -File run.ps1',
    'pwsh -EncodedCommand SQBlAHgA',
    'bash script.sh',
    // Quotes that do not wrap the whole of it are not stripped.
    "bash -c 'a' ; echo 'b'",
  ]) {
    assert.equal(shellInner(line), line)
  }
  assert.equal(shellInner(undefined), undefined)
})

test('a command summary is one clipped line of what was run', () => {
  assert.equal(commandSummary("powershell.exe -Command 'rg -n commandExecution src'"), 'rg -n commandExecution src')
  assert.equal(commandSummary('\n\n  cargo   test\nnext line'), 'cargo test')
  const long = commandSummary(`echo ${'x'.repeat(100)}`, 20)
  assert.equal(long.length, 20)
  assert.ok(long.endsWith('…'))
  assert.equal(commandSummary(undefined), '')
})
