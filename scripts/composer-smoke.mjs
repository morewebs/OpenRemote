import assert from 'node:assert/strict'
import { writeFile, mkdir } from 'node:fs/promises'
import { resolve } from 'node:path'
import { zipSync, strToU8 } from 'fflate'

// Run with Vite serving this checkout and an isolated Chrome/Edge DevTools
// port. The fixtures use real views, workers, plugin frames and the web
// provider; the account, App and peer responses contain only sample data.
const cdpOrigin = process.env.OPENREMOTE_CDP_URL || 'http://127.0.0.1:9444'
const uiOrigin = process.env.OPENREMOTE_UI_URL || 'http://127.0.0.1:5175'

const root = resolve('.scratch/composer-qa')
await mkdir(root, { recursive: true })
const target = await (await fetch(`${cdpOrigin}/json/new?about:blank`, { method: 'PUT' })).json()
const ws = new WebSocket(target.webSocketDebuggerUrl)
await new Promise(r => { ws.onopen = r })
let seq = 0
const pending = new Map(), exceptions = [], choosers = []
ws.onmessage = event => {
  const msg = JSON.parse(event.data)
  if (msg.method === 'Page.fileChooserOpened') choosers.push(msg.params)
  if (msg.method === 'Runtime.exceptionThrown') exceptions.push(msg.params.exceptionDetails.exception?.description || msg.params.exceptionDetails.text)
  if (!pending.has(msg.id)) return
  const { resolve, reject, timer } = pending.get(msg.id)
  clearTimeout(timer); pending.delete(msg.id)
  if (msg.error) reject(new Error(JSON.stringify(msg.error)))
  else resolve(msg.result)
}
const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++seq
  const timer = setTimeout(() => reject(new Error(`CDP timeout ${method}`)), 15000)
  pending.set(id, { resolve, reject, timer }); ws.send(JSON.stringify({ id, method, params }))
})
const evaluate = async expression => {
  const r = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true })
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text)
  return r.result.value
}
const pause = ms => new Promise(r => setTimeout(r, ms))
const wait = async expr => { for (let i=0;i<150;i++) { if (await evaluate(expr)) return; await pause(50) } throw new Error(`Timed out: ${expr}`) }
const click = async selector => {
  const rect = await evaluate(`(() => { const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return {x:r.left+r.width/2,y:r.top+r.height/2} })()`)
  await send('Input.dispatchMouseEvent', { type: 'mousePressed', ...rect, button: 'left', clickCount: 1 })
  await send('Input.dispatchMouseEvent', { type: 'mouseReleased', ...rect, button: 'left', clickCount: 1 })
}
const key = async (key, code = key) => {
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code })
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code })
}
const screenshot = async name => {
  const result = await send('Page.captureScreenshot', { format: 'png' })
  await writeFile(resolve(root, `${name}.png`), Buffer.from(result.data, 'base64'))
}
const navigate = async (query, width = 412, height = 844) => {
  await send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: query.includes('mobile') })
  await send('Page.navigate', { url: `${uiOrigin}/scripts/fixtures/composer-ui.html?${query}` })
  await wait(`document.querySelector('[aria-label="Add to chat"]') !== null`)
  await pause(250)
  assert.equal(await evaluate(`document.querySelector('[aria-label="Add to chat"]').disabled`), false, await evaluate('document.body.innerText'))
}
const chooseName = async name => {
  const rect = await evaluate(`(()=>{const b=[...document.querySelectorAll('.ca-add-menu .nc-item')].find(b=>b.querySelector('.nc-item-name').textContent===${JSON.stringify(name)});if(!b)return null;const r=b.getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()`)
  assert.ok(rect,name)
  await send('Input.dispatchMouseEvent',{type:'mousePressed',...rect,button:'left',clickCount:1})
  await send('Input.dispatchMouseEvent',{type:'mouseReleased',...rect,button:'left',clickCount:1})
}
const setFiles = async paths => {
  const { root } = await send('DOM.getDocument')
  const { nodeId } = await send('DOM.querySelector', { nodeId: root.nodeId, selector: 'input[type=file]' })
  await send('DOM.setFileInputFiles', { nodeId, files: paths })
}
await send('Runtime.enable')
await send('Page.enable')
try {
  for (const width of [320, 412]) {
    await navigate('mobile', width)
    await click('[aria-label="Add to chat"]')
    await wait('document.querySelector(".ca-add-menu") !== null')
    await pause(200)
    assert.deepEqual(await evaluate('[...document.querySelectorAll(".ca-add-tiles .nc-item-name")].map(e=>e.textContent)'), ['Camera','Photos','Files'])
    const bounds = await evaluate('(() => {let r=document.querySelector(".ca-add-menu").getBoundingClientRect();return {left:r.left,right:r.right,top:r.top,bottom:r.bottom,h:innerHeight,w:innerWidth}})()')
    assert.ok(bounds.left>=0 && bounds.right<=bounds.w+1 && bounds.top>=0 && bounds.bottom<=bounds.h+1, JSON.stringify(bounds))
    assert.equal(await evaluate('document.querySelector("main").closest("[inert]") !== null'), true)
    await screenshot(`mobile-${width}`)
    await key('Escape')
    await wait('document.querySelector(".ca-add-menu") === null')
    assert.equal(await evaluate('document.activeElement.getAttribute("aria-label")'), 'Add to chat')
  }
  await navigate('mobile',412,450)
  await click('[aria-label="Add to chat"]')
  await screenshot('mobile-keyboard-height')
  assert.ok(await evaluate('document.querySelector(".ca-add-menu").getBoundingClientRect().top >= 0'))
  await navigate('',1280,900)
  await click('[aria-label="Add to chat"]')
  await screenshot('desktop')
  await key('Escape')
  await click('.nc-box textarea')
  await send('Input.insertText', { text: 'Review this file' })
  await evaluate('document.querySelector(".nc-box textarea").setSelectionRange(7,11)')
  await click('[aria-label="Add to chat"]')
  await evaluate('[...document.querySelectorAll(".ca-add-menu .nc-item")].find(b=>b.textContent.includes("Reference files")).click()')
  await wait('document.querySelector(".nc-box textarea").value === "Review @ file"')
  await wait('document.querySelector(".cm-menu") !== null')
  console.log('PASS mobile/desktop layout, dismissal/focus, keyboard height, reference insertion')

  const docs = {
    'brief.md': '# Product brief\nA real document reaches the App.',
    'brief.docx': zipSync({ 'word/document.xml': strToU8('<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Hello from Word</w:t></w:r></w:p></w:body></w:document>') }),
    'budget.xlsx': zipSync({ 'xl/workbook.xml': strToU8('<workbook><sheets><sheet name="Sales" r:id="r1"/></sheets></workbook>'), 'xl/_rels/workbook.xml.rels': strToU8('<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml"/></Relationships>'), 'xl/worksheets/sheet1.xml': strToU8('<worksheet><sheetData><row><c r="A1" t="inlineStr"><is><t>Revenue</t></is></c><c r="B1"><v>42</v></c></row></sheetData></worksheet>') }),
  }
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
    '<< /Length 46 >>\nstream\nBT /F1 12 Tf 72 720 Td (Hello from PDF) Tj ET\nendstream',
  ]
  let pdf = '%PDF-1.4\n', offsets = [0]
  objects.forEach((o,i) => { offsets.push(Buffer.byteLength(pdf)); pdf+=`${i+1} 0 obj\n${o}\nendobj\n` })
  const xref = Buffer.byteLength(pdf)
  pdf+=`xref\n0 6\n0000000000 65535 f \n${offsets.slice(1).map(n=>`${String(n).padStart(10,'0')} 00000 n \n`).join('')}trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF`
  docs['sample.pdf'] = pdf
  for (const [name,data] of Object.entries(docs)) await writeFile(resolve(root,name), data)
  await navigate('mobile',412)
  await setFiles(Object.keys(docs).map(n=>resolve(root,n)))
  await wait('window.fixture.attachments("new").length===4 && window.fixture.attachments("new").every(i=>i.status!=="pending")')
  const ready = await evaluate('window.fixture.attachments("new").map(i=>({name:i.name,status:i.status,error:i.error,text:i.text}))')
  assert.ok(ready.every(i=>i.status==='ready'), JSON.stringify(ready))
  assert.ok(ready.some(i=>i.text.includes('Hello from PDF')))
  assert.ok(ready.some(i=>i.text.includes('Hello from Word')))
  assert.ok(ready.some(i=>i.text.includes('Revenue\t42')))
  await click('.ca-document-open')
  await wait('document.querySelector(".ca-document-preview") !== null')
  await pause(200)
  await screenshot('document-preview')
  await key('Escape')
  await wait('document.querySelector(".ca-document-preview") === null')
  await click('.nc-send')
  await wait('window.delivered !== undefined')
  const payload = await evaluate('window.delivered[0]')
  assert.match(payload,/Hello from PDF/)
  assert.match(payload,/Hello from Word/)
  assert.match(payload,/Revenue\t42/)
  console.log('PASS actual worker imports: Markdown, Word, spreadsheet, PDF; preview; new-chat payload')

  await navigate('mobile&noimages')
  await click('[aria-label="Add to chat"]')
  assert.deepEqual(await evaluate('[...document.querySelectorAll(".ca-add-tiles .nc-item-name")].map(e=>e.textContent)'), ['Files'])
  await screenshot('text-only-app')
  await key('Escape')
  await setFiles([resolve(root,'brief.md')])
  await wait('window.fixture.attachments("new")[0]?.status === "ready"')
  await click('.nc-send')
  await wait('window.delivered !== undefined')
  assert.match(await evaluate('window.delivered[0]'), /A real document/)
  console.log('PASS image-incompatible App still accepts documents')

  for (const query of ['chat','chat&running']) {
    await navigate(query,1280)
    await setFiles([resolve(root,'brief.md')])
    await wait('window.fixture.attachments("fixture-chat")[0]?.status === "ready"')
    await click('.nc-send')
    await wait('window.delivered !== undefined')
    assert.match(await evaluate('window.delivered[1]'), /A real document/)
  }
  console.log('PASS existing-chat send and queued-message payloads')

  await navigate('mobile')
  await send('Page.setInterceptFileChooserDialog',{enabled:true})
  for(const [name,expected] of [['Camera','camera'],['Photos','photos'],['Files','files']]){
    await click('[aria-label="Add to chat"]');await pause(200)
    const count=choosers.length
    await chooseName(name)
    for(let i=0;i<40&&choosers.length===count;i++)await pause(30)
    assert.equal(choosers.length,count+1,name)
    const {node}=await send('DOM.describeNode',{backendNodeId:choosers.at(-1).backendNodeId})
    const attrs=Object.fromEntries(Array.from({length:node.attributes.length/2},(_,i)=>node.attributes.slice(i*2,i*2+2)))
    if(expected==='camera')assert.equal(attrs.capture,'environment')
    else if(expected==='photos')assert.equal(attrs.accept,'image/*')
    else assert.match(attrs.accept,/\.pdf/)
  }
  await send('Page.setInterceptFileChooserDialog',{enabled:false})
  await click('[aria-label="Add to chat"]');await key('ArrowDown');await key('ArrowDown')
  assert.equal(await evaluate('document.activeElement.querySelector(".nc-item-name").textContent'),'Photos')
  await key('Escape')
  console.log('PASS separate real file chooser paths and arrow-key focus')

  await navigate('chat',1280)
  await click('.cv-box textarea');await send('Input.insertText',{text:'Send this draft'})
  await evaluate(`Object.defineProperty(navigator,'clipboard',{configurable:true,value:{read:()=>new Promise(r=>window.resolveClipboard=r)}})`)
  await click('[aria-label="Add to chat"]');await chooseName('Paste from clipboard')
  await wait('typeof window.resolveClipboard === "function"')
  await click('.nc-send');await wait('window.delivered !== undefined')
  await evaluate(`window.resolveClipboard([{types:['text/plain'],getType:async()=>new Blob(['Late clipboard'],{type:'text/plain'})}])`)
  await pause(150)
  assert.equal(await evaluate('window.fixture.attachments("fixture-chat").length'),0)
  console.log('PASS late clipboard result does not populate the next draft')

  await navigate('split',1280)
  await setFiles([resolve(root,'brief.md')]);await wait('window.fixture.attachments("new")[0]?.status==="ready"')
  assert.equal(await evaluate('document.querySelectorAll(".nc-box")[1].querySelectorAll(".ca-chip").length'),0)
  await evaluate(`window.pendingPrimary=window.fixture.takeFiles('new',[new File(['Slow'],'slow.txt',{type:'text/plain'})],{parse:()=>new Promise(r=>window.finishPrimary=r)});window.closeSecondComposer()`)
  await pause(100)
  await evaluate(`window.finishPrimary({text:'Slow result',format:'Text'})`)
  await wait('window.fixture.attachments("new").length===2 && window.fixture.attachments("new").every(i=>i.status==="ready")')
  console.log('PASS split composers isolate attachments and cancellation')

  await navigate('firstfail',1280)
  await click('.nc-box textarea');await send('Input.insertText',{text:'Please review'})
  await setFiles([resolve(root,'brief.md')]);await wait('window.fixture.attachments("new")[0]?.status==="ready"')
  await click('.nc-send');await wait('document.querySelector(".chatview") !== null')
  await evaluate('window.failFirst(new Error("Fixture delivery failed"))')
  await wait('document.querySelector(".cv-box textarea").value.includes("Please review") && window.fixture.attachments("fixture-chat").length===1')
  assert.equal(await evaluate('document.querySelector(".nc-send").disabled'),false)
  console.log('PASS rejected first send recovers text and document in the opened chat')

  await navigate('plugin',1280)
  await evaluate('window.selectFixtureProject()')
  await wait('document.querySelector(".plg-input") !== null')
  await click('[aria-label="Add to chat"]');await chooseName('Delayed helper')
  await wait('window.pluginResult === "clicked"')
  assert.equal(await evaluate('document.querySelector(".plg-input").dataset.state'),'active')
  await evaluate('document.querySelector("iframe").contentWindow.postMessage({releaseFixture:true},"*")')
  await wait('window.pluginResult === "inserted"')
  assert.ok(await evaluate('document.querySelector(".nc-box textarea").value.includes("Late plugin text")'))
  await evaluate('window.pluginResult = null')
  await click('[aria-label="Add to chat"]');await chooseName('Delayed helper');await wait('window.pluginResult === "clicked"')
  await evaluate('window.switchFixtureProject()');await wait('document.querySelector(".plg-input") === null')
  await evaluate('document.querySelector("iframe").contentWindow.postMessage({releaseFixture:true},"*")')
  await wait('window.pluginResult === "refused"')
  console.log('PASS real plugin frame action, shared active state and revoked project grant')

  await send('Page.navigate',{url:`${uiOrigin}/scripts/fixtures/web-provider-ui.html`})
  await wait('Boolean(window.realWebConsole)')
  await evaluate(`(async()=>{const image={kind:'image',mime:'image/png',data:'AAAA'};const created=await window.realWebConsole.createChat('',null,'claude',null,false,null,null,{deviceId:'fixture-machine',attachments:[image],returnDelivery:true});await created.delivery})()`)
  await pause(100)
  await evaluate(`(async()=>{const image={kind:'image',mime:'image/png',data:'AAAA'};await window.realWebConsole.sendPrompt('web-chat','hello',[],[image]);await window.realWebConsole.queueMessage('web-chat','queued',[],[image]);await window.realWebConsole.steerMessage('web-chat','steer',[],[image])})()`)
  const delivered=await evaluate('window.peerRequests.filter(r=>/\\/(prompts|queue|steer)$/.test(r.url))')
  assert.equal(delivered.length,4)
  assert.ok(delivered.every(r=>r.machine==='fixture-machine'&&r.body.attachments[0].data==='AAAA'))
  await evaluate('window.rejectPrompt = true')
  assert.equal(await evaluate(`(async()=>{const created=await window.realWebConsole.createChat('retry',null,'claude',null,false,null,null,{deviceId:'fixture-machine',returnDelivery:true});try{await created.delivery;return false}catch{return true}})()`),true)
  console.log('PASS real WebConsoleProvider create/photo-only, send, queue, steer through peer.fetchFor and delivery rejection')
  assert.deepEqual(exceptions, [], 'No unhandled runtime exceptions')
  console.log(`Screenshots: ${root}`)
} finally {
  await send('Target.closeTarget', { targetId: target.id }).catch(() => {})
  ws.close()
}
