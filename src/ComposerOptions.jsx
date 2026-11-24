import { useState } from 'react'
import { SlidersHorizontal } from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'

export default function ComposerOptions({ items }) {
  const [anchor, setAnchor] = useState(null)
  return <>
    <button type="button" className="nc-compact-options" title="Chat options" aria-label="Chat options" aria-haspopup="menu" aria-expanded={Boolean(anchor)}
      onClick={event => setAnchor(anchor ? null : event.currentTarget)}>
      <SlidersHorizontal size={19} />
    </button>
    {anchor && <ContextMenu anchor={anchor} anchorSide="above" compact label="Chat options"
      items={items.map(item => ({ ...item, onSelect: item.onSelect ? () => {
        // The menu returns focus before a picker captures its opener/anchor.
        requestAnimationFrame(item.onSelect)
      } : undefined }))} onClose={() => setAnchor(null)} />}
  </>
}
