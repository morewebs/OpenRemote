import { addAttachment, attachments, removeAttachment, updateAttachment, onAttachmentRemoved } from './attachments.js'
import { MAX_DOCUMENT_BYTES, MAX_DRAFT_CHARS, checkedText } from './document-limits.js'
import { imageAttachment } from './composer-attachments.js'

const jobs = new Map()
const sources = new Map()
const completions = new Map()
export const FILE_ACCEPT = '.pdf,.docx,.xlsx,.txt,.md,.markdown,.csv,.tsv,.json,.jsonl,.log,.yaml,.yml,.xml,.html,.css,.js,.jsx,.ts,.tsx,.py,.rs,.go,.java,.kt,.swift,.sh,.ps1,.sql,.toml,.ini,.ipynb,text/*'

export async function readDocument(file, { signal } = {}) {
  if (file.size > MAX_DOCUMENT_BYTES) throw new Error('This file is over 10 MB. Choose a smaller document.')
  const bytes = new Uint8Array(await file.arrayBuffer())
  if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError')
  if (typeof Worker === 'undefined') {
    const { parseDocumentBytes } = await import('./document-parse.js')
    return parseDocumentBytes({ name: file.name, type: file.type, bytes })
  }
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL('./document-worker.js', import.meta.url), { type: 'module' })
    const done = (err, result) => {
      clearTimeout(timer)
      signal?.removeEventListener('abort', abort)
      worker.terminate()
      if (err) reject(err)
      else resolve(result)
    }
    const abort = () => done(new DOMException('Cancelled', 'AbortError'))
    const timer = setTimeout(() => done(new Error('This document took too long to read. Split it into smaller files and retry.')), 25_000)
    signal?.addEventListener('abort', abort, { once: true })
    worker.onmessage = ({ data }) => {
      if (data?.type !== 'document.result') return
      done(data.error ? new Error(data.error) : null, data.result)
    }
    worker.onerror = () => done(new Error('This document could not be read. Export it again and retry.'))
    worker.postMessage({ name: file.name, type: file.type, bytes }, [bytes.buffer])
  })
}

async function prepare(key, id, file, { parse = readDocument, convert = imageAttachment } = {}) {
  return prepareItem(key, id, async (signal) => file.type?.startsWith('image/') ? convert(file) : { kind: 'document', ...await parse(file, { signal }) })
}

async function prepareItem(key, id, collect) {
  const controller = new AbortController()
  jobs.set(id, { key, controller })
  try {
    const item = await collect(controller.signal)
    if (controller.signal.aborted || !attachments(key).some(a => a.id === id)) return
    if (item.kind === 'document') {
      item.text = checkedText(item.text)
      const total = attachments(key).reduce((n, a) => n + (a.id !== id && a.kind === 'document' && a.status === 'ready' ? a.text.length : 0), 0)
      if (total + item.text.length > MAX_DRAFT_CHARS) throw new Error('These documents exceed 500,000 characters together. Remove a file and retry.')
    }
    updateAttachment(key, id, { ...item, status: 'ready', error: null })
    completions.get(id)?.()
  } catch (err) {
    if (err.name !== 'AbortError') updateAttachment(key, id, { status: 'error', error: err.message || 'This file could not be read. Retry or remove it.' })
  } finally { jobs.delete(id) }
}

export async function takeFiles(key, files, { images = true, onError = () => {}, ...options } = {}) {
  const work = []
  const ids = []
  for (const file of files) {
    if (file.type?.startsWith('image/') && !images) { onError('This App does not read photos. Choose a document instead.'); continue }
    if (attachments(key).length >= 10) { onError('A message can carry 10 attachments at most.'); break }
    const id = addAttachment({ kind: file.type?.startsWith('image/') ? 'image' : 'document', status: 'pending', name: file.name || 'Pasted photo', sourceBytes: file.size }, key)
    sources.set(id, file)
    ids.push(id)
    work.push(prepare(key, id, file, options))
  }
  await Promise.all(work)
  return ids
}

export function retryImport(key, id) {
  const source = sources.get(id)
  if (!source || jobs.has(id) || !updateAttachment(key, id, { status: 'pending', error: null })) return
  return typeof source === 'function' ? prepareItem(key, id, source) : prepare(key, id, source)
}

// Context collection participates in the same pending, removal and unmount
// lifecycle as a selected file, so sending never silently outruns the fetch.
export async function takeContext(key, pending, collect, { onReady } = {}) {
  if (attachments(key).length >= 10) throw new Error('A message can carry 10 attachments at most.')
  const id = addAttachment({ ...pending, status: 'pending' }, key)
  sources.set(id, collect)
  if (onReady) completions.set(id, onReady)
  await prepareItem(key, id, collect)
  return id
}

export function forgetImport(id) {
  jobs.get(id)?.controller.abort()
  jobs.delete(id)
  sources.delete(id)
  completions.delete(id)
}

export function cancelImports(key) {
  for (const [id, job] of jobs) if (job.key === key) { forgetImport(id); removeAttachment(key, id) }
}

onAttachmentRemoved(forgetImport)
