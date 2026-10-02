// The everywhere-composer: the same job as the big chatbox, compact
// enough to sit at the bottom of any tab. Say the thing here first.

import { useRef, useState } from 'react'
import { ArrowUp, Lightning } from '@phosphor-icons/react'
import './pill.css'

export default function PillComposer({ onSubmit }) {
  const [text, setText] = useState('')
  const ref = useRef(null)

  const submit = () => {
    if (!text.trim()) return
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
        placeholder="Ask anything, or describe a rule…"
        spellCheck={false}
      />
      <button className="pill-send" onClick={submit} disabled={!text.trim()} title="Send">
        <ArrowUp size={15} weight="bold" />
      </button>
    </div>
  )
}
