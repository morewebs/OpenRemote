import { useMemo, useState } from 'react'
import {
  CaretDown,
  Cloud,
  Desktop,
  GearSix,
  House,
  Lightning,
  MagnifyingGlass,
  Plus,
  PuzzlePiece,
  TrashSimple,
  X,
} from '@phosphor-icons/react'
import { useConsole } from './state/console.jsx'
import { sections } from './projects.js'
import { deviceName, sessionsForMode } from './cloud.js'
import { HarnessMark } from './brand-marks.jsx'
import './sidebar.css'

export default function Sidebar({ open, active, mode, onMode, onSelect, onAddProject }) {
  const { connection, sessions, chats, devices, thisDevice, projects, removeProject } = useConsole()
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
        harness: session.harness,
        workspace: session.workspace ?? null,
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
    return sorted.filter((c) => c.title.toLowerCase().includes(q))
  }, [rows, query])

  // The two sections: registered projects with their chats, then the flat
  // Chats list - every chat that belongs to no project.
  const grouped = useMemo(() => sections(projects, visible), [projects, visible])

  const toggleProject = (id) => {
    setCollapsed((c) => ({ ...c, [id]: !c[id] }))
  }

  const chatRow = (row) => (
    <button
      key={row.id}
      className={`chat-row${row.id === active ? ' active' : ''}`}
      data-status={row.status}
      onClick={() => onSelect(row.id)}
    >
      {/* the harness's mark leads - every harness, one
          sidebar, the at-a-glance signal of which agent owns
          each chat - then the title, with the status dot at
          the right edge so every status scans in one column */}
      {row.harness && <HarnessMark harness={row.harness} size={13} />}
      <span className="chat-title">{row.title}</span>
      {mode === 'cloud' && row.device && (
        <span className="chat-device">{row.device}</span>
      )}
      {mode === 'local' && row.synced && (
        <Cloud size={11} className="chat-synced" aria-label="Synced to your devices" />
      )}
      <span
        className={`dot dot--${row.offline ? 'offline' : row.status}`}
        title={row.offline ? `${row.device} is offline` : undefined}
      />
    </button>
  )

  return (
    <aside className={`sidebar${open ? '' : ' collapsed'}`}>
      <div className="sb-actions">
        {/* Local is this computer's chats; Cloud is the synced chats of
            every device the user signed in on. */}
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

      <nav className="sb-list" aria-label="Projects and chats">
        <div className="section-title sb-projects-head">
          Projects
          {/* The one way a folder becomes a project - the same flow the
              home screen's Add project button opens. */}
          <button
            type="button"
            className="sb-add-project"
            onClick={onAddProject}
            title="Add a project"
          >
            <Plus size={12} weight="bold" />
          </button>
        </div>
        {grouped.projects.length === 0 && (
          <p className="sb-empty sb-projects-empty">
            No projects yet. <button className="sb-empty-add" onClick={onAddProject}>Add a project</button> to group its chats here.
          </p>
        )}
        {grouped.projects.map(({ project, items }) => {
          const isCollapsed = !!collapsed[project.id]
          return (
            <section key={project.id} className="project">
              <button
                className="project-head"
                onClick={() => toggleProject(project.id)}
                aria-expanded={!isCollapsed}
              >
                <span className={`project-caret${isCollapsed ? ' closed' : ''}`}>
                  <CaretDown size={12} />
                </span>
                <span className="project-name">{project.folders[0].split(/[\\/]/).filter(Boolean).pop()}</span>
                <span className="project-count">{items.length}</span>
              </button>
              {/* Unregistering keeps the chats - they fall to the flat
                  list below; the folder on disk is never touched. */}
              <button
                type="button"
                className="sb-project-remove"
                title="Remove the project - its chats stay"
                onClick={() => removeProject(project.id)}
              >
                <TrashSimple size={11} />
              </button>
              {!isCollapsed && items.map(chatRow)}
            </section>
          )
        })}

        <div className="section-title sb-chats-title">Chats</div>
        {grouped.projects.length > 0 && grouped.flat.length === 0 && (
          <p className="sb-empty">No chats outside projects.</p>
        )}
        {grouped.flat.map(chatRow)}

        {rows.length === 0 && (
          <p className="sb-empty">
            {query.trim()
              ? `No chats match “${query.trim()}”.`
              : mode === 'cloud'
                ? 'No synced chats yet. Start one - it shows up on all your devices.'
                : 'No chats yet. Start one - it will appear here.'}
          </p>
        )}
        {rows.length > 0 && query.trim() && visible.length === 0 && (
          <p className="sb-empty">No chats match “{query.trim()}”.</p>
        )}
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
