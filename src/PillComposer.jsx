import { useState } from 'react'
import { ArrowUp, Lightning } from '@phosphor-icons/react'
import './pill.css'

// the everywhere-composer prototype: a short pill, fully rounded at
// both ends — same job as the big rectangular chatbox on the New
// Chat screen, but compact enough to sit at the bottom of any tab.
// Whatever the tab is, you can say the thing here first.
export default function PillComposer({ placeholder = 'Ask anything…', icon: Icon = Lightning, onSubmit }) {
  const [text, setText] = useState('')
  const canSend = text.trim().length > 0

  const send = () => {
    if (!canSend) return
    onSubmit(text.trim())
    setText('')
  }

  return (
    <div className="pill-wrap">
      <div className="pill">
        <Icon size={15} weight="light" className="pill-icon" />
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
          placeholder={placeholder}
          spellCheck={false}
          aria-label={placeholder}
        />
        <button className="pill-send" onClick={send} disabled={!canSend} title="Send">
          <ArrowUp size={14} weight="bold" />
        </button>
      </div>
    </div>
  )
}
