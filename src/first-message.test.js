import test from 'node:test'
import assert from 'node:assert/strict'
import { firstMessageDelivery } from './first-message.js'
import { webApi } from './web/webapi.js'

test('first image-only prompt reaches the actual peer transport with attachments', async () => {
  const calls = []
  const peer = { fetchFor: machine => async (url, init) => { calls.push({machine,url,body:JSON.parse(init.body)}); return Response.json({ok:true}) } }
  const image = { kind:'image', mime:'image/png', data:'AAAA' }
  await firstMessageDelivery(webApi(peer,'machine-1'),'chat-1','',[],[image],()=>{})
  assert.equal(calls[0].machine,'machine-1')
  assert.deepEqual(calls[0].body.attachments,[image])
})

test('first prompt delivery remains awaitable and reports failure for recovery', async () => {
  let rejectSend, failure
  const result = firstMessageDelivery({prompt:()=>new Promise((_, reject)=>{rejectSend=reject})},'chat-1','hello',[],[],err=>{failure=err})
  const err = new Error('Disconnected')
  rejectSend(err)
  await assert.rejects(result,/Disconnected/)
  assert.equal(failure,err)
})

test('the complete peer request includes chat references in its transport bound', async () => {
  let requests = 0
  const peer = { fetchFor: () => async () => { requests++; return Response.json({ok:true}) } }
  const image = { kind:'image', mime:'image/png', data:'A'.repeat(3_700_000) }
  await assert.rejects(webApi(peer,'machine-1').prompt('chat-1','Read this',['x'.repeat(600_000)],[image]), /4 MB|limit|large/i)
  assert.equal(requests,0)
})
