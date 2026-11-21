import assert from 'node:assert/strict'
import test from 'node:test'
import { repairMojibake } from './text-repair.js'

// UTF-8 read as windows-1252, made the way a Windows shell makes it: by a
// decoder, not by typing the damaged characters in.
const garble = (text) => new TextDecoder('windows-1252').decode(Buffer.from(text, 'utf8'))

test('UTF-8 read as windows-1252 is put back', () => {
  for (const wanted of [
    'don\u2019t',
    '\u201cClear header paragraph\u201d',
    'rate-limited \u2014 never',
    '5\u201325\u00d7 for one node',
    'sync users, \u2026',
    '\u2705 Run test command',
    '\u274c Should pass now',
    'Write \u2192 Run',
    'doc 367 \u00a75',
    'caf\u00e9 \u00fcber',
    'a \ud83d\ude80 launch',
  ]) {
    const result = repairMojibake(garble(wanted))
    assert.equal(result.text, wanted)
    assert.equal(result.repaired, true)
  }
})

test('text that is already right is left alone', () => {
  for (const fine of ['plain ascii', 'don\u2019t', 'caf\u00e9', '\u2264 2.5 s', '\u2705 done', 'a \ud83d\ude80', '']) {
    const result = repairMojibake(fine)
    assert.equal(result.text, fine)
    assert.equal(result.repaired, false)
  }
  assert.equal(repairMojibake(undefined).text, undefined)
})

test('only the garbled part of mixed text is repaired', () => {
  const mixed = `${garble('it\u2019s')} and a correct \u2264 and ${garble('\u2192')} and caf\u00e9`
  assert.equal(repairMojibake(mixed).text, 'it\u2019s and a correct \u2264 and \u2192 and caf\u00e9')
})

test('look-alike characters that are not one UTF-8 character stay as they are', () => {
  // A lead byte with nothing valid after it, a replacement character, and a
  // lead byte cut off at the end.
  for (const odd of ['\u00c3 alone', 'broken \ufffd here', 'ends on a lead \u00e2', '\u00e2\u20ac ends early']) {
    assert.equal(repairMojibake(odd).text, odd)
    assert.equal(repairMojibake(odd).repaired, false)
  }
})
