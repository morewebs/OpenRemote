// The whole browser side of OpenRemote's mesh, in one place: the device
// identity, its registration, the relay socket, and the request front the
// console can swap its `fetch` for.
//
// The protocol bytes are all in `openremote-webpeer.wasm`; this module owns the
// page's concerns (IndexedDB, WebSocket, JSON, timeouts) and nothing of Noise.

import { base64url, loadDevice, randomPrivateKey, saveDevice, forgetDevice } from './identity.js'
import { WebPeer, idBytes } from './relay.js'
import { Rpc } from './rpc.js'
import { loadWasm } from './wasm.js'

export { WebPeer, Rpc, base64url, forgetDevice, idBytes, loadDevice }

async function jsonRequest(url, token, method, body) {
  const response = await fetch(url, {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      ...(body ? { 'Content-Type': 'application/json' } : {}),
    },
    body: body ? JSON.stringify(body) : undefined,
  })
  const text = await response.text()
  const answer = text ? JSON.parse(text) : null
  if (!response.ok) {
    const said = answer?.error ?? answer?.message ?? `HTTP ${response.status}`
    throw new Error(`OpenRemote Cloud refused the request: ${said}`)
  }
  return answer
}

/**
 * Registers this browser as a device of the account. The registry answers the
 * credential the relay accepts, and the account's other devices with their
 * keys. Called once per registered device.
 */
export async function registerDevice({ api, token, device, wasm, handle, name = 'Web console', appVersion = 'web' }) {
  const publicKey = wasm.pub(handle)
  const answer = await jsonRequest(`${api.replace(/\/+$/, '')}/v1/openremote/devices`, token, 'POST', {
    id: device.deviceId,
    name,
    platform: 'web',
    kind: 'web',
    noise_pubkey: base64url(publicKey),
    app_version: appVersion,
  })
  return {
    credential: answer?.credential ?? null,
    accountId: answer?.account?.id ?? answer?.account?.account_id ?? null,
    devices: answer?.devices ?? answer?.device_list ?? [],
  }
}

/** The account's devices, with the keys the links need. */
export async function listDevices({ api, token }) {
  const answer = await jsonRequest(`${api.replace(/\/+$/, '')}/v1/openremote/devices`, token, 'GET')
  return answer?.devices ?? []
}

/**
 * The browser as one of the user's OpenRemote devices.
 *
 * `api` is the gateway's base (`https://api.moreweb.space`), `token` the
 * console's account token, `wasmUrl` where `openremote-webpeer.wasm` is served
 * from. Answers the peer, the device record, and a `fetch` for one machine.
 */
export async function createWebPeer({ api, token, wasmUrl, name = 'Web console', appVersion = 'web', events = () => {} }) {
  const wasm = await loadWasm(wasmUrl)
  let device = await loadDevice()
  if (!device) {
    device = { deviceId: crypto.randomUUID(), privateKey: Array.from(randomPrivateKey()), accountId: null, credential: null }
    await saveDevice(device)
  }
  const handle = wasm.newPeer(Uint8Array.from(device.privateKey))
  if (!handle) throw new Error('this browser\'s device key is not a usable key')
  if (!device.credential) {
    const registered = await registerDevice({ api, token, device, wasm, handle, name, appVersion })
    device = { ...device, credential: registered.credential, accountId: registered.accountId }
    await saveDevice(device)
  }
  const peer = new WebPeer({
    api,
    wasm,
    handle,
    deviceId: device.deviceId,
    accountId: device.accountId,
    credential: device.credential,
    events,
  })
  peer.knowDevices(await listDevices({ api, token }))
  await peer.connect()
  return {
    peer,
    device,
    /** A `fetch` that routes the console's daemon calls to one machine. */
    fetchFor: (machineId) => peer.fetchTo(machineId),
    /** Forget this browser's device; the registry entry stays until revoked. */
    forget: async () => {
      peer.close()
      await forgetDevice()
    },
    close: () => peer.close(),
  }
}
