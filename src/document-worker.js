import { parseDocumentBytes } from './document-parse.js'

self.onmessage = async ({ data }) => {
  try { self.postMessage({ type: 'document.result', result: await parseDocumentBytes(data) }) }
  catch (err) { self.postMessage({ type: 'document.result', error: err.message || 'This document could not be read. Export it again and retry.' }) }
}
