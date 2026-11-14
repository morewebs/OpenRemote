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
      const content = await page.getTextContent()
      const text = content.items.map(item => `${item.str ?? ''}${item.hasEOL ? '\n' : ' '}`).join('').trim()
      if (text) readable = true
      const block = `Page ${i}\n${text}`
      length += block.length
      if (length > 200_000) checkedText('x'.repeat(200_001))
      blocks.push(block)
      page.cleanup()
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
