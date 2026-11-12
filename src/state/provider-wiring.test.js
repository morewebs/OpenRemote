// What a unit test cannot reach from outside: the provider is a hook, so
// these four wiring facts are read from its source, the same way the other
// wiring tests read the views. Each one is the load-bearing line of a fix
// whose failure the reporter would feel, not a description of the file.

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

const source = readFileSync(new URL('./console.jsx', import.meta.url), 'utf8')

const between = (from, to) => {
  const start = source.indexOf(from)
  const end = source.indexOf(to, start + from.length)
  assert.ok(start >= 0 && end > start, `${from} .. ${to} not found in console.jsx`)
  return source.slice(start, end)
}

test('a status report for the same daemon does not reconnect', () => {
  // A respawn on the same port, or a retry that found the daemon already up,
  // used to tear down every live stream and replay every transcript. The
  // comparison has to come before the reconnect, or it decides nothing.
  const body = between('const applyDaemonInfo', 'const retryDaemon')
  const guard = body.indexOf('sameDaemon(apiRef.current, info.url, info.token)')
  const connect = body.indexOf('connect(info.url, info.token)')
  assert.ok(guard >= 0, 'the report is compared with the daemon in hand')
  assert.ok(connect > guard, 'and the comparison runs before any reconnect')
})

test('an answer from a daemon the console has left is dropped', () => {
  // The model catalog is the one that sticks: it is fetched once per key, so
  // a stale answer would be offered for the rest of the session.
  const body = between('const modelsFor', 'The custom model endpoints')
  assert.match(body, /if \(!stillCurrent\(api\)\) return list/)
})

test('a reconnect resumes from the fold cursor, never a second one', () => {
  const body = between('const openStream', 'A new connection')
  assert.doesNotMatch(body, /stream\.cursor/, 'one cursor, and it is the fold\'s')
  assert.match(body, /follow\(chatsRef\.current\[id\]\?\.foldedSeq \?\? null\)/)
})

test('pointing at another daemon clears what the last one answered', () => {
  const connect = between('const connect', 'const disconnect')
  const disconnect = between('const disconnect', 'One way in for everything')
  assert.match(connect, /resetDaemonData\(\)/, 'a switch resets the daemon-owned lists')
  assert.match(disconnect, /resetDaemonData\(\)/, 'and so does a disconnect')
})

test('a frame of stream events is folded once, against one copy', () => {
  // A replay hands a whole history over in one tick, and the fold used to copy
  // the chat and its timeline for every event. The events are queued per chat
  // and folded once per animation frame; that the result is the same
  // transcript is reducer.test.js's comparison, and this pins the wiring.
  assert.match(source, /import \{ chatFromSession, firstPromptText, foldEvents, mergeSessionIntoChat, titleFrom, vanishedChatIds \} from '\.\/reducer\.js'/)
  const apply = between('const apply = (event) => {', 'const follow = (after) => {')
  assert.match(apply, /queueEvent\(id, event\)/, 'an event is queued, not folded where it lands')
  assert.doesNotMatch(apply, /setChats/, 'and no state is published per event')
  assert.doesNotMatch(apply, /\bfoldEvent\(/, 'nor copied per event')
  const flush = between('const flushEvents = useCallback', 'const queueEvent = useCallback')
  assert.match(flush, /foldEvents\(\{ \.\.\.chat, timeline: \[\.\.\.chat\.timeline\] \}, events\)/, 'the frame folds one batch against one copy')
  assert.match(flush, /setChats\(\(current\) =>/, 'and publishes once')
  // One frame is scheduled until it runs, and the queue is per chat.
  const queue = between('const queueEvent = useCallback', '// One stream per watched chat')
  assert.match(queue, /pendingRef\.current/)
  assert.match(queue, /if \(frameRef\.current === null\) frameRef\.current = nextFrame\(flushEvents\)/)
  // A closed stream drops what the frame had not folded: the cursor still sits
  // at the last folded seq, so a later open asks for those events again.
  assert.match(between('stream.close = () => {', 'return stream'), /pendingRef\.current\.delete\(id\)/)
  // A connection change cancels the pending frame and empties the queue.
  assert.match(source, /cancelFrame\(frameRef\.current\)/)
  assert.match(source, /pendingRef\.current\.clear\(\)/)
})
test('the stream retry is mirrored for the screen and cleared three ways', () => {
  // The stream object owns the retry - its delay and its timer - and the state
  // is a mirror the views read. This pins the four moments: set when a close
  // schedules another attempt, cleared when the stream answers, when it gives
  // up (a deleted session), and when it is closed for good.
  assert.match(source, /const \[reconnecting, setReconnecting\] = useState\(\{\}\)/)
  const window = (needle, size = 800) => {
    const at = source.indexOf(needle)
    assert.ok(at >= 0, `${needle} is in console.jsx`)
    return source.slice(at, at + size)
  }
  assert.match(window('const apply = (event) => {', 400), /markReconnecting\(id, false\)/, 'an answer clears it')
  const onClose = window('onClose: (_cursor, err) => {', 2400)
  assert.match(onClose, /markReconnecting\(id, true\)/, 'a scheduled retry is shown')
  assert.match(onClose, /if \(sessionGone\(err\)\) \{[\s\S]{0,200}markReconnecting\(id, false\)/, 'a give-up is not shown as a retry')
  // A refusal no reconnect can fix stops the ladder too, and says why in the transcript
  // rather than promising a reconnection for ever.
  assert.match(onClose, /const refused =[\s\S]{0,400}markReconnecting\(id, false\)/, 'a refusal is not shown as a retry')
  assert.match(onClose, /type: 'daemon\.error'/, 'and it leaves a note')
  assert.match(window('stream.close = () => {', 700), /markReconnecting\(id, false\)/, 'closing clears it')
  // Exposed on the context value and its dependency list.
  assert.match(source, /^\s+reconnecting,$/m)
  // The mirror decides nothing: the stream still owns the ladder.
  assert.match(source, /stream\.retryDelay = nextRetryDelay\(stream\.retryDelay\)/)
  assert.match(source, /stream\.retry = setTimeout/)
  // The setter keeps the state object when nothing changed, so an idle beat
  // does not re-render every consumer of the context.
  assert.match(source, /if \(on\) return current\[id\] \? current : \{ \.\.\.current, \[id\]: true \}/)
  assert.match(source, /if \(!current\[id\]\) return current/)
})