import { useMemo, useState } from 'react'
import { CaretDown, Cloud, Desktop, GearSix, House, Lightning, MagnifyingGlass, Plus, PuzzlePiece, X } from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { workspaceName } from './state/reducer.js'
import { deviceName, sessionsForMode } from './cloud.js'
import { isMobile } from './platform.js'
import './sidebar.css'

export default function Sidebar({ open, active, mode, onMode, onSelect }) {
  const { connection, sessions, chats, devices, thisDevice } = useConsole()
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState({})

  // Local: everything that runs on this computer. Cloud: every synced chat,
  // named by the device it runs on when that isn't this one.
  const rows = useMemo(() => {
    return sessionsForMode(sessions, mode, thisDevice).map((session) => {
      const elsewhere = session.executor && session.executor !== thisDevice
      const device = elsewhere ? (devices ?? []).find((d) => d.id === session.executor) : null
      return {
        id: session.id,
        title: chats[session.id]?.title ?? session.title ?? 'A new task',
        status: chats[session.id]?.status ?? 'idle',
        project: workspaceName(session.workspace),
        updatedAt: session.updated_at ?? 0,
        device: elsewhere ? deviceName(devices, session.executor) : null,
        // A copy's last status is stale while its device is away.
        offline: elsewhere && !device?.online,
        synced: Boolean(session.executor),
      }
    })
  }, [sessions, chats, mode, devices, thisDevice])

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
        {/* Local is this computer's chats; Cloud is the synced chats of
            every device the user signed in on. A phone has only Cloud. */}
        {!isMobile && (
        <div className="mode-switch" role="group" aria-label="Connection mode">
          <button
            className={mode === 'local' ? 'on' : ''}
            aria-pressed={mode === 'local'}
            onClick={() => onMode('local')}
          >
            <House size={13} />
            Local
          </button>
          <button
            className={mode === 'cloud' ? 'on' : ''}
            aria-pressed={mode === 'cloud'}
            onClick={() => onMode('cloud')}
          >
            <Cloud size={13} />
            Cloud
          </button>
        </div>
        )}
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
        {/* Plugins and automations act on this computer, so they belong
            to Local; Machines belongs to Cloud. The tiles reveal and
            collapse with the mode switch (the grid-rows transition). */}
        <div className={`machines-reveal${mode === 'cloud' ? ' closed' : ''}`}>
          <div className="machines-clip">
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
          </div>
        </div>
        <div className={`machines-reveal${mode === 'cloud' ? '' : ' closed'}`}>
          <div className="machines-clip">
            <button
              className={`side-tile${active === 'machines' ? ' on' : ''}`}
              onClick={() => onSelect('machines')}
            >
              <Desktop size={17} />
              Machines
            </button>
          </div>
        </div>
      </div>

      <nav className="sb-list" aria-label="Projects">
        <div className="section-title">Projects</div>
        {groups.length === 0 && (
          <p className="sb-empty">
            {query.trim()
              ? `No chats match “${query.trim()}”.`
              : mode === 'cloud'
                ? 'No synced chats yet. Start one - it shows up on all your devices.'
                : 'No chats yet. Start one - it will appear here grouped by folder.'}
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
                    <span
                      className={`dot dot--${row.offline ? 'offline' : row.status}`}
                      title={row.offline ? `${row.device} is offline` : undefined}
                    />
                    <span className="chat-title">{row.title}</span>
                    {mode === 'cloud' && row.device && (
                      <span className="chat-device">{row.device}</span>
                    )}
                    {mode === 'local' && row.synced && (
                      <Cloud size={11} className="chat-synced" aria-label="Synced to your devices" />
                    )}
                  </button>
                ))}
            </section>
          )
        })}
      </nav>

      <footer className="sb-foot">
        {/* Settings is a view like the others - the gear navigates to it. */}
        <button
          className={`settings-btn${active === 'settings' ? ' on' : ''}`}
          onClick={() => onSelect('settings')}
        >
          <GearSix size={15} />
          Settings
        </button>
      </footer>
    </aside>
  )
}
