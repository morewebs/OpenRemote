import React, { StrictMode, useState } from 'react'
import { createRoot } from 'react-dom/client'
import NewChat from '../../src/NewChat.jsx'
import ChatView from '../../src/ChatView.jsx'
import { ConsoleContext } from '../../src/state/console.jsx'
import { KeyboardProvider } from '../../src/keyboard.jsx'
import '../../src/base.css'
import '../../src/composer.css'
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import { attachments, addAttachment } from '../../src/attachments.js'
import { outgoing } from '../../src/composer-attachments.js'
import { takeFiles } from '../../src/document-input.js'
import { PluginProvider } from '../../src/plugins-host.jsx'
window.fixture = { attachments, outgoing, takeFiles, addAttachment }
const params = new URLSearchParams(location.search)
localStorage.removeItem('openremote-presets')
const harnesses = [{ id: 'claude', name: 'Claude Code', available: true, honors: { attachments: !params.has('noimages') }, permission_modes: [{ w: 'default', l: 'Default' }], default_permission: 'default' }]
const models = [{ model: 'claude-sonnet', display_name: 'Claude Sonnet', is_default: true, reasoning_efforts: ['low', 'medium', 'high'] }]
const chat = { id: 'fixture-chat', title: 'Composer verification', harness: 'claude', status: params.has('running') ? 'running' : 'idle', running: false, executor: 'desktop', workspace: '/fixture', timeline: [], queue: [], model: 'claude-sonnet', context: { used: 0, window: 200000 } }
const noop = () => {}
const capture = (...args) => { window.delivered = args; return Promise.resolve(params.has('firstfail') ? { id: 'fixture-chat', delivery: new Promise((_, reject) => { window.failFirst = reject }) } : 'fixture-chat') }
const projects = [{ id:'project-a', name:'Alpha', folders:['/alpha'] }, { id:'project-b', name:'Beta', folders:['/beta'] }]
const plugin = { id:'fixture@local', name:'Project helper', runtime:'js', scope:'project', project:'project-a', enabled:true, powers:[], entry:'main.js', contributes:{composer_inputs:[{id:'late',label:'Delayed helper',icon:'microphone'}]} }
const pluginSource = `openremote.composer.onClick(async ({composer})=>{await openremote.composer.setState('late',{state:'active',label:'Waiting'},{composer});parent.postMessage({fixture:'clicked'},'*');const finish=async e=>{if(!e.data?.releaseFixture)return;removeEventListener('message',finish);try{await openremote.composer.insert('Late plugin text',{composer});await openremote.composer.setState('late',{state:'idle'},{composer});parent.postMessage({fixture:'inserted'},'*')}catch(e){parent.postMessage({fixture:'refused'},'*')}};addEventListener('message',finish)});`
addEventListener('message', e => { if (e.data?.fixture) window.pluginResult = e.data.fixture })
const context = {
  connection: { state: 'connected' }, capabilities: { harnesses }, chats: [chat], sessions: [chat], projects, devicesAnswered: true,
  devices: [{ id: 'desktop', name: 'Workstation', kind: 'machine', online: true, platform: 'windows' }], thisDevice: params.has('mobile') ? 'phone' : 'desktop', cloud: { state: 'signed-in' }, plugins: { installed: params.has('plugin') ? [plugin] : [] },
  api: { pluginFile: async () => ({type:'application/javascript',bytes:new TextEncoder().encode(pluginSource).buffer}) },
  capabilitiesFor: async () => ({ harnesses }), apiFor: () => ({ projects: async () => projects }), modelsFor: async () => models,
  commandsFor: async () => ({ commands: [{ name: 'review', description: 'Review your changes', kind: 'command', available: true }], agents: [] }), filesFor: async () => ({ entries: [{ path: 'README.md', kind: 'file' }] }),
  endpointsFor: async () => [], endpointModelsFor: async () => [], createChat: capture, sendPrompt: capture, queueMessage: capture, steerMessage: capture,
  updateChatSettings: capture, renameChat: noop,
}
function Fixture() {
  const [mounted, setMounted] = useState(true)
  const [view,setView] = useState(params.has('chat') ? 'chat' : 'new')
  const [project,setProject] = useState({id:'project-a',nonce:1})
  const [second,setSecond] = useState(true)
  window.unmountComposer = () => setMounted(false)
  window.switchFixtureProject = () => setProject({id:'project-b',nonce:2})
  window.selectFixtureProject = () => setProject({id:'project-a',nonce:3})
  window.closeSecondComposer = () => setSecond(false)
  return <ConsoleContext.Provider value={context}><KeyboardProvider><PluginProvider><main style={{ height: '100dvh', display: 'flex', flexDirection: 'column', background: '#121212' }}>
    <header style={{ padding: '18px 22px', color: '#ddd', fontSize: 16 }}>OpenRemote</header>
    {mounted && (view==='chat' ? <ChatView chat={chat} onOpenPlugins={() => { window.pluginsOpened = true }} /> : <NewChat presetProject={params.has('plugin') ? project : null} mode={params.has('mobile') ? 'cloud' : 'local'} onOpen={id => { window.opened = id; if(params.has('firstfail'))setView('chat') }} onOpenPlugins={() => { window.pluginsOpened = true }} />)}
    {params.has('split') && second && <NewChat composerScope="second" mode="local" onOpen={noop} focused={false} />}
  </main></PluginProvider></KeyboardProvider></ConsoleContext.Provider>
}
createRoot(document.getElementById('root')).render(<StrictMode><Fixture /></StrictMode>)
