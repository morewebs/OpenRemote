import { useEffect, useRef, useState } from 'react'
import { ArrowUp } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { HarnessIcon } from './BrandIcon.jsx'
import ContextRing from './ContextRing.jsx'
import { MODELS, harnessName } from './catalog.js'
import { PROJECTS, contextWindow } from './world.js'
import './chatview.css'
import './composer.css'

const TOOL_LABEL = { pending: 'Needs a decision', ok: 'Done', denied: 'Denied', failed: 'Failed' }

function machineName(chat, devices) {
  if (!chat.machine) return 'this computer'
  return devices.find((d) => d.id === chat.machine)?.name ?? chat.machine
}

export default function ChatView({ chat, devices, harnesses, onSend, onResolve, onHarness, onModel }) {
  const [text, setText] = useState('')
  const [picker, setPicker] = useState(null)
  const scrollRef = useRef(null)
  const composerRef = useRef(null)
  const pending = chat.messages.some((m) => m.role === 'tool' && m.state === 'pending')
  const canSend = text.trim().length > 0 && !pending

  const currentHarness = harnesses.find((h) => h.id === chat.harness)
  const modelList = MODELS[chat.harness] ?? []
  const currentModel =
    modelList.find((m) => m.id === chat.model) ?? modelList[0] ?? { name: chat.modelLabel ?? chat.model }
  const project = PROJECTS.find((p) => p.id === chat.project)
  const where = machineName(chat, devices)
  const harnessLabel = currentHarness?.name ?? harnessName(chat.harness)

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [chat.messages, chat.status])

  // the composer floats over the transcript; its height varies as the
  // textarea grows. Publish it as --cv-clear so the thread's bottom
  // padding always keeps the last message above the box, never under it
  useEffect(() => {
    const composer = composerRef.current
    if (!composer) return
    const set = () => {
      const root = composer.parentElement
      if (root) root.style.setProperty('--cv-clear', `${Math.ceil(composer.offsetHeight)}px`)
    }
    set()
    const ro = new ResizeObserver(set)
    ro.observe(composer)
    return () => ro.disconnect()
  }, [])

  const send = () => {
    if (!canSend) return
    onSend(chat.id, text.trim())
    setText('')
  }

  const place = project?.path ?? project?.name ?? chat.project
  const blocks = []
  let rail = null
  for (const message of chat.messages) {
    if (message.role === 'user') {
      rail = null
      blocks.push({ kind: 'user', id: message.id, message })
    } else {
      if (!rail) {
        rail = { kind: 'rail', id: message.id, items: [] }
        blocks.push(rail)
      }
      rail.items.push(message)
    }
  }
  if (chat.status === 'running') {
    const last = blocks.at(-1)
    if (last?.kind === 'rail') last.running = true
    else blocks.push({ kind: 'rail', id: 'running', items: [], running: true })
  }

  return (
    <div className="chatview">
      <header className="cv-head">
        <h1>{chat.title}</h1>
        <p className="cv-facts">
          {chat.status !== 'idle' && <span className={`cv-dot cv-dot--${chat.status}`} />}
          <span className="cv-where">{place}</span>
          <span className="cv-fact">{harnessLabel}</span>
          <span className="cv-fact">{currentModel.name}</span>
          <span className="cv-fact">{where}</span>
          {chat.status === 'waiting' && <span className="cv-fact">Needs your decision</span>}
          {chat.approvals?.length > 0 && (
            <span className="cv-fact">{chat.approvals.join(', ')} allowed for this chat</span>
          )}
        </p>
      </header>
      <div className="cv-scroll" ref={scrollRef}>
        <div className="cv-thread">
          {blocks.map((block) => {
            if (block.kind === 'user') {
              return (
                <div key={block.id} className="cv-user">
                  {block.message.text}
                </div>
              )
            }
            return (
              <div key={block.id} className="cv-rail">
                {block.items.map((m) => {
                  if (m.role === 'tool') {
                    return (
                      <div key={m.id} className={`cv-node cv-node--${m.state}`}>
                        <div className="cv-tool-line">
                          <span className="cv-tool-name">{m.name}</span>
                          {m.state !== 'ok' && <span className="cv-tool-state">{TOOL_LABEL[m.state] ?? m.state}</span>}
                        </div>
                        {(m.arg || m.result) && (
                          <div className="cv-io">
                            {m.arg && (
                              <div className="cv-io-row">
                                <span className="cv-io-label">In</span>
                                <code>{m.arg}</code>
                              </div>
                            )}
                            {m.result && (
                              <div className="cv-io-row">
                                <span className="cv-io-label">Out</span>
                                <code>{m.result}</code>
                              </div>
                            )}
                          </div>
                        )}
                        {m.state === 'pending' && (
                          <div className="cv-tool-actions">
                            <button type="button" className="primary" onClick={() => onResolve(chat.id, m.id, 'once')}>
                              Allow once
                            </button>
                            <button type="button" onClick={() => onResolve(chat.id, m.id, 'session')}>
                              Allow for this chat
                            </button>
                            <button type="button" onClick={() => onResolve(chat.id, m.id, 'deny')}>
                              Deny
                            </button>
                          </div>
                        )}
                      </div>
                    )
                  }
                  if (m.role === 'note') {
                    return (
                      <p key={m.id} className="cv-node cv-note">
                        {m.text}
                      </p>
                    )
                  }
                  return (
                    <p key={m.id} className="cv-node cv-agent">
                      {m.text}
                    </p>
                  )
                })}
                {block.running && <p className="cv-node cv-node--run cv-running">With {harnessLabel} · {where}</p>}
              </div>
            )
          })}
        </div>
      </div>

      <div className="cv-composer" ref={composerRef}>
        <div className="cv-box">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                send()
              }
            }}
            placeholder={pending ? 'Decide on the command above' : 'Steer the session'}
            disabled={pending}
            spellCheck={false}
          />
          <div className="cv-foot">
            <div className="nc-pickers">
              <button
                className="nc-meta"
                onClick={openPicker('model')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'model'}
                title="Model"
              >
                {currentModel.name}
              </button>
              <span className="nc-via">via</span>
              <button
                className="nc-meta"
                onClick={openPicker('harness')}
                aria-haspopup="listbox"
                aria-expanded={picker?.kind === 'harness'}
                title="Harness"
              >
                <HarnessIcon harness={currentHarness} size={13} />
                {harnessLabel}
              </button>
              {picker?.kind === 'model' && (
                <PickerMenu
                  label="Model"
                  searchPlaceholder="Search models"
                  items={modelList}
                  groups={null}
                  selectedId={currentModel.id}
                  onChoose={(id) => {
                    onModel(chat.id, id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                />
              )}
              {picker?.kind === 'harness' && (
                <PickerMenu
                  label="Harness"
                  searchPlaceholder="Search harnesses"
                  items={harnesses}
                  groups
                  selectedId={chat.harness}
                  onChoose={(id) => {
                    onHarness(chat.id, id)
                    setPicker(null)
                  }}
                  onClose={() => setPicker(null)}
                  anchor={{ left: picker.x, top: picker.y }}
                  renderIcon={(h) => <HarnessIcon harness={h} size={14} />}
                />
              )}
            </div>
            <div className="nc-send-group">
              <ContextRing used={chat.contextUsed ?? 0} window={contextWindow(chat.model)} />
              <button className="nc-send" onClick={send} disabled={!canSend} title="Send">
                <ArrowUp size={15} weight="bold" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}
