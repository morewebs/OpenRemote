import { createPortal } from 'react-dom'
import { ArrowsLeftRight, FloppyDisk, SquaresFour } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { paneCount } from './split.js'
import { loadLayouts, offerLayouts, SAVED_LAYOUTS_NONE } from './layouts.js'
import { WORKSPACE_PRESETS, layoutFits, presetLayout } from './workspace-layout.js'
import './composer-add.css'
import './workspace-layouts.css'

function PresetShape({ preset, count }) {
  const slots = preset === 'grid' ? 4 : preset === 'main-stack' ? Math.max(3, count) : Math.max(2, count)
  return (
    <span
      className={`workspace-preset-shape workspace-preset-shape--${preset}`}
      style={{ '--workspace-slots': slots, '--workspace-stack': slots - 1 }}
      aria-hidden="true"
    >
      {Array.from({ length: slots }, (_, index) => <i key={index} />)}
    </span>
  )
}

export default function WorkspaceLayouts({
  layout, width, height, anchor, currentChat,
  onPreset, onEqualize, onSave, onRestore, onClose,
}) {
  const count = paneCount(layout)
  const names = { columns: 'Columns', rows: 'Rows', 'main-stack': 'Main + stack', grid: 'Grid' }
  const items = WORKSPACE_PRESETS.map(preset => {
    const candidate = presetLayout(layout, preset)
    const fits = layoutFits(candidate, width, height)
    return {
      id: `preset:${preset}`, name: names[preset], preset, tile: true, group: 'Presets',
      disabled: !fits,
      note: fits ? null : 'There is not enough room for this layout. Enlarge the workspace first.',
    }
  })
  if (count > 1) items.push(
    { id: 'equalize', name: 'Equalize panes', group: 'Actions' },
    { id: 'save', name: 'Save layout…', group: 'Actions' },
  )
  const offers = offerLayouts(loadLayouts(), { currentChat })
  items.push(...offers.map((offer, index) => {
    const fits = offer.ok && layoutFits(offer.plan.layout, width, height)
    return {
      id: `saved:${index}`, name: offer.name, group: 'Saved', veiled: true,
      detail: offer.countLine, plan: offer.plan, disabled: !fits,
      note: offer.note ?? (fits ? null : 'There is not enough room for this saved layout. Enlarge the workspace first.'),
    }
  }))
  if (!offers.length) items.push({ id: 'none', name: SAVED_LAYOUTS_NONE, group: 'Saved', disabled: true })

  const choose = id => {
    const item = items.find(candidate => candidate.id === id)
    if (!item || item.disabled) return
    onClose()
    if (item.preset) onPreset(item.preset)
    else if (id === 'equalize') onEqualize()
    else if (id === 'save') onSave()
    else if (item.plan) onRestore(item.plan)
  }

  return createPortal(
    <div className="workspace-layout-menu">
      <PickerMenu
        label="Workspace layout"
        presentation="actions"
        hideSearch
        groups={[]}
        items={items}
        anchor={{ left: anchor.x, top: anchor.y }}
        place={anchor.above ? 'above' : 'below'}
        onChoose={choose}
        onClose={onClose}
        note={<><strong>Workspace layout</strong><span>{count} {count === 1 ? 'pane' : 'panes'}</span></>}
        renderIcon={item => item.preset
          ? <PresetShape preset={item.preset} count={count} />
          : item.id === 'equalize' ? <ArrowsLeftRight size={17} />
            : item.id === 'save' ? <FloppyDisk size={17} />
              : item.plan ? <SquaresFour size={17} /> : null}
        renderSubline={item => item.note && !item.tile ? <span className="nc-item-sub">{item.note}</span> : null}
        footer={<p className="workspace-layout-hint">Drag a pane header to move it.<br />Double-click a divider to equalize panes.</p>}
      />
    </div>,
    document.body,
  )
}
