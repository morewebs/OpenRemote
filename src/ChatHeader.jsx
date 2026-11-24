import { useState } from 'react'
import {
  CloudArrowUp, Copy, DotsThree, FolderSimple, GitBranch, Info,
  PencilSimple, Trash, UserCircle,
} from '@phosphor-icons/react'
import ContextMenu from './ContextMenu.jsx'
import { chatRights, menuItems, metaItems, notices } from './chat-header.js'
import { canCopyChat } from './chat-markdown.js'
import { isMobile } from './platform.js'
import { usePrivacyMode } from './privacy.js'
import { chatTitle } from './shape.js'
import './chathead.css'

const MENU_ICONS = {
  rename: PencilSimple, copy: Copy, details: Info, account: UserCircle,
  sync: CloudArrowUp, delete: Trash,
}

function MetaItem({ item, privacy }) {
  const { kind, label, title, state, device } = item
  const Icon = kind === 'folder' ? FolderSimple : kind === 'branch' ? GitBranch : null
  return (
    <span className={`ch-meta-item ch-meta-${kind}${device ? ' has-device' : ''}`} title={privacy ? undefined : title}>
      {Icon && <Icon size={13} aria-hidden="true" />}
      {state && <span className={`ch-dot ch-dot--${state}`} aria-hidden="true" />}
      <span className={`ch-meta-text${Icon ? ' pv' : ''}`}>
        {label}{device && <> on <span className="pv">{device}</span></>}
      </span>
    </span>
  )
}

export default function ChatHeader({
  chat, app, device, gitPlace, access, web, cloudReady, accountDisconnected,
  onRename, onDetails, onCopy, onAccount,
  onSync, onDelete, onMakeMachine, error, onClearError,
}) {
  const privacy = usePrivacyMode()
  const [anchor, setAnchor] = useState(null)
  const [confirmation, setConfirmation] = useState(null)
  const [pendingNotice, setPendingNotice] = useState(null)
  const title = chatTitle(chat)
  const rights = chatRights(chat, {
    access, web, cloudReady, canRename: onRename, canManageAccount: onAccount,
  })
  const ask = (kind) => {
    onClearError()
    setConfirmation(kind)
  }
  const actions = {
    rename: onRename, copy: onCopy, details: onDetails, account: onAccount,
    sync: () => ask('sync'), delete: () => ask('delete'),
  }
  const noticeActions = { machine: onMakeMachine, sync: onSync, delete: onDelete }
  const runNotice = async (kind) => {
    if (pendingNotice) return
    setPendingNotice(kind)
    try {
      // Keep the question and the retry target in place when the call fails.
      if (await noticeActions[kind]()) setConfirmation((current) => current === kind ? null : current)
    } finally {
      setPendingNotice(null)
    }
  }

  return (
    <header className="ch">
      <h1 className="ch-title">
        {onRename ? (
          <button type="button" className="ch-title-button" onClick={onRename}
            title={privacy ? 'Rename chat' : title}>
            <span className="ch-title-text pv">{title}</span>
            <PencilSimple size={13} aria-hidden="true" />
          </button>
        ) : <span className="ch-title-text pv" title={privacy ? undefined : title}>{title}</span>}
      </h1>
      <div className="ch-meta">
        {metaItems(chat, { app, device, gitPlace }).map((item) => (
          <MetaItem key={item.kind} item={item} privacy={privacy} />
        ))}
      </div>
      <div className="ch-actions">
        <button type="button" className="ch-action ch-more" aria-label="Chat actions" title="Chat actions"
          aria-haspopup="menu" aria-expanded={Boolean(anchor)}
          onClick={(event) => setAnchor(anchor ? null : event.currentTarget)}>
          <DotsThree size={20} weight="bold" aria-hidden="true" />
        </button>
      </div>
      {notices(chat, rights, { confirmation, error }).map(({ kind, text, action, cancel }) => (
        <div key={kind} className={`ch-notice ch-notice--${kind}`}>
          <span className={kind === 'error' || kind === 'failure' ? 'pv' : undefined}
            role={kind === 'error' ? 'alert' : action ? 'status' : undefined}>{text}</span>
          {kind === 'machine' ? (
            <button type="button" className="ch-action" disabled={Boolean(pendingNotice)} onClick={() => runNotice(kind)}>
              <span>Let <span className="pv">{device}</span> run chats</span>
            </button>
          ) : action && (
            <div className="ch-notice-actions">
              <button type="button" className="ch-action" disabled={Boolean(pendingNotice)} onClick={() => ask(null)}>{cancel}</button>
              <button type="button" className={`ch-action${kind === 'delete' ? ' ch-danger' : ''}`}
                disabled={Boolean(pendingNotice)} onClick={() => runNotice(kind)}>{action}</button>
            </div>
          )}
        </div>
      ))}
      {anchor && (
        <ContextMenu anchor={anchor} label="Chat actions" compact={!isMobile} onClose={() => setAnchor(null)}
          items={menuItems(rights, { copyable: canCopyChat(chat), accountDisconnected }).map((item) => {
            if (item.divider) return item
            const Icon = MENU_ICONS[item.id]
            return { ...item, icon: <Icon size={14} aria-hidden="true" />, onSelect: actions[item.id] }
          })}
        />
      )}
    </header>
  )
}
