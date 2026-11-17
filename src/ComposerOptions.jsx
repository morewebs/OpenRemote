import { useState } from 'react'
import { SlidersHorizontal } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'

export default function ComposerOptions({ items }) {
  const [at, setAt] = useState(null)
  return <>
    <button type="button" className="nc-compact-options" title="Chat options" aria-label="Chat options" aria-haspopup="menu" aria-expanded={Boolean(at)}
      onClick={e => { const r = e.currentTarget.getBoundingClientRect(); setAt(current => current ? null : { x: r.right - 220, y: r.top - items.length * 30 - 14 }) }}>
      <SlidersHorizontal size={19} />
    </button>
    {at && <ContextMenu x={at.x} y={at.y} compact label="Chat options"
      items={items.map(item => ({ ...item, onSelect: item.onSelect ? () => {
        // The menu returns focus before a picker captures its opener/anchor.
        requestAnimationFrame(item.onSelect)
      } : undefined }))} onClose={() => setAt(null)} />}
  </>
}
