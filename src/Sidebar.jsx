import { useMemo, useState } from 'react'
import { CaretDown, Cloud, GearSix, House, Lightning, MagnifyingGlass, Plus, PuzzlePiece, WifiHigh, X } from '@phosphor-icons/react'
import { PROJECTS } from './world.js'
import './sidebar.css'

export default function Sidebar({ open, active, onSelect, mode, onModeChange, chats }) {
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState({})

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return chats
    return chats.filter((c) => c.title.toLowerCase().includes(q))
  }, [query, chats])

  const groups = []
  const byProject = new Map()
  for (const chat of visible) {
    const label = PROJECTS.find((p) => p.id === chat.project)?.name ?? chat.project
    let group = byProject.get(label)
    if (!group) {
      group = { label, items: [] }
      byProject.set(label, group)
      groups.push(group)
    }
    group.items.push(chat)
  }

  const toggleProject = (project) => {
    setCollapsed((c) => ({ ...c, [project]: !c[project] }))
  }

  return (
    <aside className={`sidebar${open ? '' : ' collapsed'}`}>
      <div className="sb-actions">
        <div className="mode-switch" role="group" aria-label="Connection mode">
          <button
            className={mode === 'local' ? 'on' : ''}
            aria-pressed={mode === 'local'}
            onClick={() => onModeChange('local')}
          >
            <House size={13} />
            Local
          </button>
          <button
            className={mode === 'cloud' ? 'on' : ''}
            aria-pressed={mode === 'cloud'}
            onClick={() => onModeChange('cloud')}
          >
            <Cloud size={13} />
            Cloud
          </button>
        </div>
        <button className="side-tile" onClick={() => onSelect('new')}>
          <Plus size={17} weight="bold" />
          New chat
        </button>
        <label className="side-tile search">
          <MagnifyingGlass size={17} className="search-icon" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search"
            spellCheck={false}
          />
          {query && (
            <button className="search-clear" onClick={() => setQuery('')} title="Clear">
              <X size={12} weight="bold" />
            </button>
          )}
        </label>
        <button className={`side-tile${active === 'plugins' ? ' on' : ''}`} onClick={() => onSelect('plugins')}>
          <PuzzlePiece size={17} />
          Plugins
        </button>
        <button className={`side-tile${active === 'automations' ? ' on' : ''}`} onClick={() => onSelect('automations')}>
          <Lightning size={17} />
          Automations
        </button>
        <button className={`side-tile${active === 'devices' ? ' on' : ''}`} onClick={() => onSelect('devices')}>
          <WifiHigh size={17} />
          Machines
        </button>
      </div>

      <nav className="sb-list" aria-label="Chats">
        <div className="section-title">Chats</div>
        {groups.map((group) => {
          const isCollapsed = !!collapsed[group.label]
          return (
            <section key={group.label} className="project">
              <button
                className="project-head"
                onClick={() => toggleProject(group.label)}
                aria-expanded={!isCollapsed}
              >
                <span
                  className={`project-caret${isCollapsed ? ' closed' : ''}`}
                >
                  <CaretDown size={12} />
                </span>
                <span className="project-name">{group.label}</span>
                <span className="project-count">{group.items.length}</span>
              </button>
              {!isCollapsed &&
                group.items.map((chat) => (
                  <button
                    key={chat.id}
                    className={`chat-row${chat.id === active ? ' active' : ''}`}
                    data-status={chat.status}
                    onClick={() => onSelect(chat.id)}
                  >
                    <span className={`dot dot--${chat.status}`} />
                    <span className="chat-title">{chat.title}</span>
                  </button>
                ))}
            </section>
          )
        })}
      </nav>

      <footer className="sb-foot">
        <button className={`settings-btn${active === 'settings' ? ' on' : ''}`} onClick={() => onSelect('settings')}>
          <GearSix size={15} />
          Settings
        </button>
      </footer>
    </aside>
  )
}

