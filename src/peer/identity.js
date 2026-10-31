// The browser device's identity: 32 random bytes from the page's own entropy,
// kept in IndexedDB along with the device id, the account it was registered
// with, and the credential the relay accepts.
//
// The private key never leaves this browser and is never sent anywhere: the
// registry is told only its public half. Losing the IndexedDB block means a new
// device, which is why "forget" is an explicit call and not a cleanup path.

const DB_NAME = 'openremote-webpeer'
const DB_VERSION = 1
const STORE = 'device'
const KEY = 'this-browser'

function open() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE)) request.result.createObjectStore(STORE)
    }
    request.onerror = () => reject(request.error)
    request.onsuccess = () => resolve(request.result)
  })
}

function run(mode, work) {
  return open().then(
    (db) =>
      new Promise((resolve, reject) => {
        const tx = db.transaction(STORE, mode)
        const request = work(tx.objectStore(STORE))
        request.onerror = () => reject(request.error)
        request.onsuccess = () => resolve(request.result)
        tx.oncomplete = () => db.close()
      }),
  )
}

/** The device this browser already is, or null before it has one. */
export function loadDevice() {
  if (typeof indexedDB === 'undefined') return Promise.resolve(null)
  return run('readonly', (store) => store.get(KEY)).catch(() => null)
}

/** Keeps a device: the whole record, so a reload is the same device. */
export function saveDevice(device) {
  if (typeof indexedDB === 'undefined') return Promise.resolve()
  return run('readwrite', (store) => store.put(device, KEY))
}

/** Forgets this browser's device. The registry entry stays until revoked. */
export function forgetDevice() {
  if (typeof indexedDB === 'undefined') return Promise.resolve()
  return run('readwrite', (store) => store.delete(KEY))
}

/** 32 random bytes from `crypto.getRandomValues`, the wasm handshake's key. */
export function randomPrivateKey() {
  const key = new Uint8Array(32)
  globalThis.crypto.getRandomValues(key)
  return key
}

/** base64url without padding, the encoding the registry uses for keys. */
export function base64url(bytes) {
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}
