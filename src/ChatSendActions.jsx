import { useState } from 'react'
import { ArrowUp, CaretDown, Play, Stop } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'
import { useHint, useKeyLabel } from './keyboard.jsx'
import { isMobile } from './platform.js'

function StopReply({ onInterrupt, onEnd }) {
  const [anchor, setAnchor] = useState(null)
  const interruptHint = useHint('interrupt')
  const endKey = useKeyLabel('stop')
  return (
    <div className="cv-stop-group">
      <button type="button" className="nc-send cv-run-button" onClick={onInterrupt}
        title={interruptHint('Stop the reply and hold any queued messages; the chat stays open')}>
        <Stop size={12} weight="fill" aria-hidden="true" /> Stop
      </button>
      <button type="button" className="cv-run-more" aria-label="Run actions" title="Run actions"
        aria-haspopup="menu" aria-expanded={Boolean(anchor)}
        onClick={(event) => setAnchor(anchor ? null : event.currentTarget)}>
        <CaretDown size={12} weight="bold" aria-hidden="true" />
      </button>
      {anchor && (
        <ContextMenu anchor={anchor} anchorSide="above" label="Run actions" compact={!isMobile}
          items={[{ label: 'End run', icon: <Stop size={14} aria-hidden="true" />, hint: endKey, onSelect: onEnd }]}
          onClose={() => setAnchor(null)} />
      )}
    </div>
  )
}

export default function ChatSendActions({ runAction, showSend, canSend, sendLabel, sendHint, onSend, onInterrupt, onEnd, onResume }) {
  return (
    <div className="cv-send-actions">
      {showSend && (
        <button type="button" className="nc-send" onClick={onSend} disabled={!canSend} aria-label={sendLabel} title={sendHint}>
          <ArrowUp size={15} weight="bold" aria-hidden="true" />
        </button>
      )}
      {runAction === 'interrupt' && <StopReply onInterrupt={onInterrupt} onEnd={onEnd} />}
      {runAction === 'resume' && (
        <button type="button" className="nc-send cv-run-button" onClick={onResume} title="Pick this chat up again">
          <Play size={12} weight="fill" aria-hidden="true" /> Resume
        </button>
      )}
    </div>
  )
}
