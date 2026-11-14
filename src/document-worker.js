import { parseDocumentBytes } from './document-parse.js'

self.onmessage = async ({ data }) => {
  try { self.postMessage({ result: await parseDocumentBytes(data) }) }
  catch (err) { self.postMessage({ error: err.message || 'This document could not be read. Export it again and retry.' }) }
}
