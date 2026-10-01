import { useState } from 'react'
import { ArrowUp, House } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { HarnessIcon } from './BrandIcon.jsx'
import { MODELS, deviceIcon, harnessName } from './catalog.js'
import { PROJECTS } from './world.js'
import './newchat.css'
import './composer.css'

const DEVICE_STATUS = { online: 'Online', offline: 'Offline', waiting: 'Waiting' }

export default function NewChat({ mode, onCreate, devices, harnesses, defaults, chats, onOpen }) {
  const [text, setText] = useState('')
  const [harness, setHarness] = useState(defaults?.harness ?? 'cc')
  const [model, setModel] = useState(defaults?.model ?? 'sonnet')
  const [device, setDevice] = useState(defaults?.deviceId ?? devices[0]?.id)
  const [project, setProject] = useState(defaults?.project ?? PROJECTS[0].id)
  const [picker, setPicker] = useState(null)
  const canSend = text.trim().length > 0
  const currentHarness = harnesses.find((h) => h.id === harness) ?? harnesses[0]
  const modelList = MODELS[harness] ?? []
  const currentModel = modelList.find((m) => m.id === model) ?? modelList[0]
  const currentDevice = devices.find((d) => d.id === device) ?? devices[0]
  const currentProject = PROJECTS.find((p) => p.id === project) ?? PROJECTS[0]
  const resume = (chats ?? []).filter((c) => c.status === 'running' || c.status === 'waiting')

  const openPicker = (kind) => (e) => setPicker({ kind, x: e.clientX, y: e.clientY })

  const chooseHarness = (id) => {
    setHarness(id)
    setModel(MODELS[id][0].id)
    setPicker(null)
  }

  const send = () => {
    if (!canSend || !currentModel) return
    onCreate(text, harness, currentModel.id, currentDevice?.id, currentProject.id)
  }

  return (
    <div className="newchat">
      <h1 className="nc-greeting">What are we working on?</h1>
      <div className="nc-box">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
          placeholder="Describe the task"
          autoFocus
          spellCheck={false}
        />
        <div className="nc-foot">
          <div className="nc-pickers">
            <button
              className="nc-meta"
              onClick={openPicker('model')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'model'}
              title="Model"
            >
              {currentModel?.name ?? 'Model'}
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
              {currentHarness?.name ?? harness}
            </button>
            {picker?.kind === 'model' && (
              <PickerMenu
                label="Model"
                searchPlaceholder="Search models"
                items={modelList}
                groups={null}
                selectedId={currentModel?.id}
                onChoose={(id) => {
                  setModel(id)
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
                selectedId={harness}
                onChoose={chooseHarness}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderIcon={(h) => <HarnessIcon harness={h} size={14} />}
              />
            )}
          </div>
          <div className="nc-send-group">
            <button className="nc-send" onClick={send} disabled={!canSend} title="Start task">
              <ArrowUp size={15} weight="bold" />
            </button>
          </div>
        </div>
      </div>
      <div className="nc-under">
      <div className="nc-below">
        <div className="nc-device">
          <button
            className="nc-meta"
            onClick={openPicker('project')}
            aria-haspopup="listbox"
            aria-expanded={picker?.kind === 'project'}
            title={currentProject.path}
          >
            {currentProject.name}
          </button>
          {picker?.kind === 'project' && (
            <PickerMenu
              label="Project"
              searchPlaceholder="Search projects"
              items={PROJECTS}
              groups={null}
              selectedId={currentProject.id}
              onChoose={(id) => {
                setProject(id)
                setPicker(null)
              }}
              onClose={() => setPicker(null)}
              anchor={{ left: picker.x, top: picker.y }}
              renderTrailing={(p) => <span className="nc-item-path">{p.path}</span>}
            />
          )}
        </div>
        {mode === 'cloud' && currentDevice ? (
          <div className="nc-device">
            <button
              className="nc-meta"
              onClick={openPicker('device')}
              aria-haspopup="listbox"
              aria-expanded={picker?.kind === 'device'}
              title={DEVICE_STATUS[currentDevice.status]}
            >
              {(() => {
                const Icon = deviceIcon(currentDevice.kind)
                return <Icon size={13} weight="light" />
              })()}
              {currentDevice.name}
              <span className={`nc-dev-dot nc-dev-dot--${currentDevice.status}`} />
            </button>
            {picker?.kind === 'device' && (
              <PickerMenu
                label="Device"
                searchPlaceholder="Search devices"
                items={devices}
                groups={null}
                selectedId={device}
                onChoose={(id) => {
                  setDevice(id)
                  setPicker(null)
                }}
                onClose={() => setPicker(null)}
                anchor={{ left: picker.x, top: picker.y }}
                renderIcon={(d) => {
                  const Icon = deviceIcon(d.kind)
                  return <Icon size={14} weight="light" />
                }}
                renderTrailing={(d) => (
                  <span className={`nc-dev-dot nc-dev-dot--${d.status}`} title={DEVICE_STATUS[d.status]} />
                )}
              />
            )}
          </div>
        ) : (
          <span className="nc-meta nc-static">
            <House size={13} weight="light" />
            This computer
          </span>
        )}
      </div>
      {resume.length > 0 && (
        <div className="nc-resume">
          <div className="nc-resume-label">In progress</div>
          {resume.map((c) => {
            const where = c.machine ? (devices.find((d) => d.id === c.machine)?.name ?? c.machine) : 'this computer'
            return (
              <button key={c.id} type="button" onClick={() => onOpen(c.id)}>
                <span className={`dot dot--${c.status}`} />
                <span className="nc-resume-title">{c.title}</span>
                <span className="nc-resume-meta">
                  {c.status === 'waiting' ? 'Needs a decision' : harnessName(c.harness)} · {where}
                </span>
              </button>
            )
          })}
        </div>
      )}
      </div>
    </div>
  )
}
