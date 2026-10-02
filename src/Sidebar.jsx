import { useMemo, useState } from 'react'
import { CaretDown, GearSix, Lightning, MagnifyingGlass, Plus, PuzzlePiece, WifiHigh, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { workspaceName } from './state/reducer.js'
import './sidebar.css'

export default function Sidebar({ open, active, onSelect, onReplayOnboarding }) {
  const { connection, sessions, chats } = useConsole()
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState({})

  const rows = useMemo(() => {
    return (sessions ?? []).map((session) => ({
      id: session.id,
      title: chats[session.id]?.title ?? 'A new task',
      status: chats[session.id]?.status ?? 'idle',
      project: workspaceName(session.workspace),
      updatedAt: session.updated_at ?? 0,
    }))
  }, [sessions, chats])

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase()
    const sorted = [...rows].sort((a, b) => b.updatedAt - a.updatedAt)
    if (!q) return sorted
    return sorted.filter((c) => c.title.toLowerCase().includes(q) || c.project.toLowerCase().includes(q))
  }, [rows, query])

  const groups = []
  const byProject = new Map()
  for (const row of visible) {
    let group = byProject.get(row.project)
    if (!group) {
      group = { label: row.project, items: [] }
      byProject.set(row.project, group)
      groups.push(group)
    }
    group.items.push(row)
  }

  const toggleProject = (project) => {
    setCollapsed((c) => ({ ...c, [project]: !c[project] }))
  }

  return (
    <aside className={`sidebar${open ? '' : ' collapsed'}`}>
      <div className="sb-actions">
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
      </div>

      <nav className="sb-list" aria-label="Chats">
        <button
          className={`chat-row nav-tile${active === 'machines' ? ' active' : ''}`}
          onClick={() => onSelect('machines')}
        >
          <WifiHigh size={15} className="nav-tile-icon" />
          Machines
        </button>
        <button
          className={`chat-row nav-tile${active === 'plugins' ? ' active' : ''}`}
          onClick={() => onSelect('plugins')}
        >
          <PuzzlePiece size={15} className="nav-tile-icon" />
          Plugins
        </button>
        <button
          className={`chat-row nav-tile${active === 'automations' ? ' active' : ''}`}
          onClick={() => onSelect('automations')}
        >
          <Lightning size={15} className="nav-tile-icon" />
          Automations
        </button>
        <div className="section-title">Chats</div>
        {groups.length === 0 && (
          <p className="sb-empty">No chats yet. Start one — it will appear here grouped by folder.</p>
        )}
        {groups.map((group) => {
          const isCollapsed = !!collapsed[group.label]
          return (
            <section key={group.label} className="project">
              <button
                className="project-head"
                onClick={() => toggleProject(group.label)}
                aria-expanded={!isCollapsed}
              >
                <span className={`project-caret${isCollapsed ? ' closed' : ''}`}>
                  <CaretDown size={12} />
                </span>
                <span className="project-name">{group.label}</span>
                <span className="project-count">{group.items.length}</span>
              </button>
              {!isCollapsed &&
                group.items.map((row) => (
                  <button
                    key={row.id}
                    className={`chat-row${row.id === active ? ' active' : ''}`}
                    data-status={row.status}
                    onClick={() => onSelect(row.id)}
                  >
                    <span className={`dot dot--${row.status}`} />
                    <span className="chat-title">{row.title}</span>
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
