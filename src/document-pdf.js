import { getDocument, GlobalWorkerOptions } from 'pdfjs-dist/build/pdf.mjs'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { checkedText } from './document-limits.js'

GlobalWorkerOptions.workerSrc = workerUrl

export async function pdfText(bytes) {
  const task = getDocument({ data: bytes, isEvalSupported: false, useWorkerFetch: false, disableFontFace: true, disableAutoFetch: true, stopAtErrors: true })
  try {
    const doc = await task.promise
    if (doc.numPages > 500) throw new Error('This PDF has more than 500 pages. Split it into smaller files.')
    const blocks = []
    let length = 0
    let readable = false
    for (let i = 1; i <= doc.numPages; i++) {
      const page = await doc.getPage(i)
      const reader = page.streamTextContent().getReader()
      const pieces = []
      length += `Page ${i}\n`.length + 2
      try {
        while (true) {
          const { value, done } = await reader.read()
          if (done) break
          for (const item of value.items) {
            const text = `${item.str ?? ''}${item.hasEOL ? '\n' : ' '}`
            length += text.length
            if (length > 200_000) throw new Error('This document exceeds 200,000 extracted characters. Split it into smaller files.')
            if (text.trim()) readable = true
            pieces.push(text)
          }
        }
        blocks.push(`Page ${i}\n${pieces.join('').trim()}`)
      } finally {
        await reader.cancel().catch(() => {})
        page.cleanup()
      }
    }
    if (!readable) checkedText('')
    return blocks.join('\n\n')
  } catch (err) {
    if (err.name === 'PasswordException') throw new Error('This PDF is password protected. Export an unlocked copy and retry.')
    if (['InvalidPDFException', 'UnknownErrorException'].includes(err.name)) throw new Error('This PDF could not be read. Export it again and retry.')
    throw err
  } finally {
    await task.destroy()
  }
}
