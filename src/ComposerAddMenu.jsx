import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { Camera, ImageSquare, Paperclip, ClipboardText, At, Globe, Command, Brain, PlugsConnected, CaretRight, Plus, Selection, CursorClick } from '@phosphor-icons/react'
import PickerMenu from './PickerMenu.jsx'
import { composerActions } from './composer-actions.js'
import { hasTauri, isMobile } from './platform.js'
import { PluginGlyph } from './plugin-icons.jsx'
import { getState, subscribe, call } from './browser-store.js'
import { browserContextActions, collectBrowserContext } from './browser-composer-actions.js'
import { requestBrowser } from './browser-placement.js'
import { composerTarget } from './composer-insert.js'
import { attachments } from './attachments.js'
import { takeContext } from './document-input.js'
import { sharedWith, tellDaemon } from './browser-share.js'
import { errorCopy } from './error-copy.js'
import './composer-add.css'

const ICONS = { camera: Camera, photos: ImageSquare, files: Paperclip, clipboard: ClipboardText, reference: At, browser: Globe, commands: Command, effort: Brain, plugin: PlugsConnected, region: Selection, element: CursorClick }

export default function ComposerAddMenu({ composerKey, takesImages, disabled, pickPhoto, pickFile, pickCamera, pasteClipboard, onReference, onCommands, onPlugins, onEffort, effort, onError, pluginActions = [] }) {
  const [open, setOpen] = useState(false)
  const [screen, setScreen] = useState('root')
  const [tabKey, setTabKey] = useState(null)
  const browser = useSyncExternalStore(subscribe, getState)
  const pages = browser.tabs.filter(t => t.native && /^https?:/i.test(t.url ?? ''))
  const tab = pages.find(t => t.key === tabKey)
  const trigger = useRef(null)
  const close = useCallback(() => { setOpen(false); setScreen('root') }, [])
  useEffect(() => { if (disabled) close() }, [disabled, close])
  const items = screen === 'pages' ? pages.map(t => ({ id: t.key, name: t.title || t.url, detail: new URL(t.url).host, icon: 'browser', veiled: true })) : screen === 'browser' && tab ? browserContextActions({ features: browser.features, images: takesImages, mobile: isMobile }).map(i => ({ ...i, detail: tab.title || new URL(tab.url).host })) : composerActions({ images: takesImages, mobile: isMobile, clipboard: Boolean(navigator.clipboard?.read || navigator.clipboard?.readText), browser: hasTauri, pluginActions, effort: onEffort ? effort || 'Choose effort' : null, plugins: !isMobile && Boolean(onPlugins) })
  const choose = async (id) => {
    if (disabled) return
    const selected = items.find(i => i.id === id)
    if (!selected || selected.disabled) return
    if (screen === 'pages') { setTabKey(id); setScreen('browser'); return }
    if (screen === 'browser' && tab) {
      const target = composerTarget(composerKey)
      close()
      if (!target) return
      if (id === 'region' || id === 'element') {
        requestBrowser({ chatId: composerKey, tab: tab.key, action: id, target })
        return
      }
      try {
        const itemId = await takeContext(composerKey, { name: tab.title || 'Web page', kind: id === 'screenshot' ? 'image' : 'document' }, () => collectBrowserContext(id, tab, call))
        if (target.isLive() && attachments(composerKey).some(i => i.id === itemId && i.status === 'ready')) sharedWith(composerKey, tellDaemon)
      } catch (err) { if (target.isLive()) onError?.(errorCopy(err)) }
      return
    }
    if (id === 'browser') {
      if (!pages.length) { close(); requestBrowser({ chatId: composerKey }); return }
      if (pages.length === 1) { setTabKey(pages[0].key); setScreen('browser') }
      else setScreen('pages')
      return
    }
    close()
    // Keep file/clipboard requests inside the user activation that chose them.
    const run = { photos: pickPhoto, files: pickFile, camera: pickCamera, clipboard: pasteClipboard, reference: onReference, commands: onCommands, plugins: onPlugins, effort: onEffort }[id]
    if (run) run()
    else selected.onChoose?.()
  }
  return <>
    <button ref={trigger} type="button" className="nc-meta ca-attach" data-picker="add" disabled={disabled} onClick={() => setOpen(v => !v)} aria-label="Add to chat" aria-haspopup={isMobile ? 'dialog' : 'menu'} aria-expanded={open} title="Add to chat">
      <Plus size={21} />
    </button>
    {open && <PickerMenu label={screen === 'root' ? 'Add to chat' : screen === 'pages' ? 'Choose a page' : 'Browser context'} presentation="actions" items={items} groups={[]} hideSearch portal place="above" anchor={{ left: trigger.current?.getBoundingClientRect().left ?? 16, top: trigger.current?.getBoundingClientRect().top ?? 16 }} onClose={close} onBack={screen !== 'root' ? () => setScreen(screen === 'browser' && pages.length > 1 ? 'pages' : 'root') : null} onChoose={choose}
      renderIcon={item => { const Icon = ICONS[item.icon] || PlugsConnected; return <span className="ca-add-icon">{item.input ? <PluginGlyph name={item.input.icon} size={22} /> : <Icon size={22} />}</span> }}
      renderTrailing={item => <>{item.state?.state === 'busy' && <span className="ca-action-status">Working</span>}{item.state?.state === 'active' && <span className="ca-action-status">Active</span>}{item.hint && <span className="ca-add-hint">{item.hint === 'submenu' ? <CaretRight size={15} /> : item.hint}</span>}</>}
      footer={<p className="ca-add-footnote">{screen === 'root' ? 'PDF, Word, spreadsheets and text up to 10 MB' : 'Added only to this chat when you send'}</p>}
    />}
  </>
}
