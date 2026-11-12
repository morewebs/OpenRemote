// The everywhere-composer: the same job as the big chatbox, compact
// enough to sit at the bottom of any tab. Say the thing here first.

import { useRef, useState } from 'react'
import { ArrowUp, Lightning } from '@phosphor-icons/react'
import { messageBound } from './send-bound.js'
import './pill.css'

export default function PillComposer({ onSubmit }) {
  const [text, setText] = useState('')
  const ref = useRef(null)
  // What the daemon would refuse before it is sent: the ceiling is a body limit
  // answered with an empty 413, so a person would get a number rather than a
  // sentence naming the size and the limit. The composer warns instead.
  const bound = messageBound(text)

  const submit = () => {
    if (!text.trim() || bound) return
    // The text leaves the field as soon as it is handed over: the caller puts it
    // in the conversation first, and answers its own failure there, so a copy kept
    // here would be the same words twice.
    onSubmit(text)
    setText('')
  }

  return (
    <div className="pill" ref={ref}>
      <Lightning size={15} className="pill-icon" />
      <textarea
        value={text}
        rows={1}
        onChange={(e) => {
          setText(e.target.value)
          const el = e.target
          el.style.height = 'auto'
          el.style.height = `${Math.min(el.scrollHeight, 120)}px`
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey) {
            e.preventDefault()
            submit()
          }
        }}
        placeholder="Describe a rule: when it runs and what the chat does"
        spellCheck={false}
      />
      <button
        className="pill-send"
        onClick={submit}
        disabled={!text.trim() || Boolean(bound)}
        // A disabled control says why it cannot act, and the enabled one names the
        // key that would have sent it: the hover was "Send", which only repeated
        // the arrow.
        title={
          bound
            ? bound
            : text.trim()
              ? 'Send (Enter). Shift+Enter makes a new line.'
              : 'Type a message first.'
        }
      >
        <ArrowUp size={15} weight="bold" />
      </button>
      {bound && (
        <p className="pill-bound" role="status">
          {bound}
        </p>
      )}
    </div>
  )
}
