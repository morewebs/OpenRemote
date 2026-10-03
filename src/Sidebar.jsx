import { useMemo, useState } from 'react'
import { CaretDown, Cloud, GearSix, House, Lightning, MagnifyingGlass, Plus, PuzzlePiece, X } from '@phosphor-icons/react'
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
        {/* The mode switch keeps the prototype's shape with Local
            pressed; Cloud arrives with remote check-in and stays quiet
            until then. */}
        <div className="mode-switch" role="group" aria-label="Connection mode">
          <button className="on" aria-pressed="true">
            <House size={13} />
            Local
          </button>
          <button disabled title="Cloud arrives with remote machine check-in">
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
        <button
          className={`side-tile${active === 'plugins' ? ' on' : ''}`}
          onClick={() => onSelect('plugins')}
        >
          <PuzzlePiece size={17} />
          Plugins
        </button>
        <button
          className={`side-tile${active === 'automations' ? ' on' : ''}`}
          onClick={() => onSelect('automations')}
        >
          <Lightning size={17} />
          Automations
        </button>
        {/* The Machines tile returns with remote check-in — until then its
            Add-a-machine flow would point at an install command that
            doesn't exist, and no dead UI is the rule. */}
      </div>

      <nav className="sb-list" aria-label="Projects">
        <div className="section-title">Projects</div>
        {groups.length === 0 && (
          <p className="sb-empty">
            {query.trim()
              ? `No chats match “${query.trim()}”.`
              : 'No chats yet. Start one — it will appear here grouped by folder.'}
          </p>
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
