import test from 'node:test'
import assert from 'node:assert/strict'
import { zipSync, strToU8 } from 'fflate'
import { parseDocumentBytes, documentType } from './document-parse.js'
import { takeFiles, takeContext, cancelImports, readDocument } from './document-input.js'
import { attachments, clearAttachments, removeAttachment } from './attachments.js'
import { outgoing } from './composer-attachments.js'

const file = (text, name = 'notes.md', type = 'text/plain') => new File([text], name, { type })
const parse = (text, name = 'notes.md') => parseDocumentBytes({ name, bytes: strToU8(text) })
const office = (entries) => zipSync(Object.fromEntries(Object.entries(entries).map(([n, v]) => [n, strToU8(v)])))

test('UTF-8 documents retain their name and readable text; binary and legacy files are rejected', async () => {
  assert.deepEqual(await parse('Hello 🌍\nTwo'), { text: 'Hello 🌍\nTwo', format: 'Text' })
  assert.equal(documentType('report.csv', ''), 'text')
  assert.equal(documentType('main.rs', ''), 'text')
  await assert.rejects(parse('binary', 'program.exe'), /supported|not supported/i)
  await assert.rejects(parse('word', 'old.doc'), /supported|not supported/i)
  await assert.rejects(parse('\x00\x01hello'), /binary|UTF-8/i)
  await assert.rejects(parse('   '), /readable text/i)
  await assert.rejects(parse('a'.repeat(200_001)), /200,000/)
})

test('DOCX paragraphs and table cells become readable bounded text', async () => {
  const bytes = office({ 'word/document.xml': '<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Hello &amp; world</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>' })
  const out = await parseDocumentBytes({ name: 'brief.docx', bytes })
  assert.match(out.text, /Hello & world/)
  assert.match(out.text, /A\s+B/)
  assert.equal(out.format, 'Word')
  await assert.rejects(parseDocumentBytes({ name: 'bad.docx', bytes: strToU8('bad') }), /read|corrupt|invalid/i)
})

test('XLSX includes every sheet name, shared strings, empty columns and inline strings', async () => {
  const bytes = office({
    'xl/workbook.xml': '<workbook><sheets><sheet name="Sales" r:id="r1"/><sheet name="Costs" r:id="r2"/></sheets></workbook>',
    'xl/_rels/workbook.xml.rels': '<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml"/><Relationship Id="r2" Target="worksheets/sheet2.xml"/></Relationships>',
    'xl/sharedStrings.xml': '<sst><si><t>Revenue</t></si></sst>',
    'xl/worksheets/sheet1.xml': '<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="C1"><v>42</v></c></row></sheetData></worksheet>',
    'xl/worksheets/sheet2.xml': '<worksheet><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Hosting</t></is></c></row></sheetData></worksheet>',
  })
  const out = await parseDocumentBytes({ name: 'budget.xlsx', bytes })
  assert.match(out.text, /Sheet: Sales\nRevenue\t\t42/)
  assert.match(out.text, /Sheet: Costs\nHosting/)
  assert.equal(out.format, 'Spreadsheet')
})

test('Office expansion and XML entity declarations are rejected before parsing', async () => {
  const bytes = office({ 'word/document.xml': '<!DOCTYPE x [<!ENTITY x "boom">]><document><p>&x;</p></document>' })
  await assert.rejects(parseDocumentBytes({ name: 'entity.docx', bytes }), /XML|declaration/i)
  const bomb = office({ 'word/document.xml': 'x'.repeat(21 * 1024 * 1024) })
  await assert.rejects(parseDocumentBytes({ name: 'large.docx', bytes: bomb }), /expand|large/i)
})

test('imports reserve slots before async conversion and removal prevents resurrection', async () => {
  const key = 'import-race'
  clearAttachments(key)
  let finish
  const work = takeFiles(key, [file('a')], { parse: () => new Promise(r => { finish = r }) })
  assert.equal(attachments(key)[0].status, 'pending')
  removeAttachment(key, attachments(key)[0].id)
  finish({ text: 'a', format: 'Text' })
  await work
  assert.deepEqual(attachments(key), [])
})

test('document import keeps errors visible, supports retry, and cancellation drops unfinished items', async () => {
  const key = 'import-errors'
  clearAttachments(key)
  await takeFiles(key, [file('hello'), file('x', 'bad.exe')])
  assert.equal(attachments(key)[0].status, 'ready')
  assert.equal(attachments(key)[1].status, 'error')
  let finish
  const pending = takeFiles(key, [file('late')], { parse: () => new Promise(r => { finish = r }) })
  cancelImports(key)
  finish({ text: 'late', format: 'Text' })
  await pending
  assert.equal(attachments(key).length, 2)
  clearAttachments(key)
})

test('simultaneous imports never exceed ten items or 500,000 extracted characters', async () => {
  const key = 'import-limits'
  clearAttachments(key)
  const errors = []
  await Promise.all(Array.from({ length: 12 }, (_, i) => takeFiles(key, [file('hello', `${i}.txt`)], { onError: e => errors.push(e) })))
  assert.equal(attachments(key).length, 10)
  assert.equal(errors.length, 2)
  clearAttachments(key)
  await takeFiles(key, [file('a'.repeat(200_000)), file('b'.repeat(200_000)), file('c'.repeat(200_000))])
  assert.equal(attachments(key)[2].status, 'error')
  assert.match(attachments(key)[2].error, /500,000/)
  clearAttachments(key)
  await takeFiles(key, [new File([new Uint8Array(10 * 1024 * 1024 + 1)], 'huge.txt')])
  assert.match(attachments(key)[0].error, /10 MB/)
  clearAttachments(key)
})

test('documents become named reference blocks for every App, pending and failed items block sending', () => {
  const item = { kind: 'document', name: 'brief.md', status: 'ready', text: 'A useful brief', format: 'Text' }
  const out = outgoing('Summarize', [item], { images: false })
  assert.equal(out.error, null)
  assert.match(out.text, /Summarize[\s\S]*brief.md[\s\S]*A useful brief/)
  assert.deepEqual(out.attachments, [])
  assert.match(outgoing('hello', [{ ...item, status: 'pending' }]).error, /prepar/i)
  assert.match(outgoing('hello', [{ ...item, status: 'error', error: 'unreadable' }]).error, /remove|retry/i)
})

test('the assembled escaped prompt and images are bounded before delivery', () => {
  const item = { kind: 'document', status: 'ready', name: 'large.txt', text: '\n'.repeat(1_100_000) }
  assert.match(outgoing('', [item]).error, /large|long|limit|MB/i)
  const image = { kind: 'image', dataUrl: `data:image/png;base64,${'A'.repeat(3_700_000)}` }
  assert.equal(outgoing('Hi', [image]).error, null, 'local image uploads use the daemon image route limit')
  assert.match(outgoing('"'.repeat(100_000), [image], { remote: true }).error, /large|limit|MB/i)
})

test('PDF worker internal messages cannot finish the document import early', async () => {
  const original = globalThis.Worker
  let terminated = 0
  // PDF.js initializes its own message channel in a worker. Exercise the
  // browser adapter with that extra traffic before our extraction result.
  globalThis.Worker = class {
    postMessage() {
      queueMicrotask(() => {
        this.onmessage({ data: { sourceName: 'worker', action: 'ready' } })
        this.onmessage({ data: { type: 'document.result', result: { text: 'PDF reference', format: 'PDF' } } })
      })
    }
    terminate() { terminated++ }
  }
  try {
    assert.deepEqual(await readDocument(file('pdf', 'test.pdf')), { text: 'PDF reference', format: 'PDF' })
    assert.equal(terminated, 1)
  } finally { if (original) globalThis.Worker = original; else delete globalThis.Worker }
})

test('browser collection reserves a pending chip and cancellation discards its late result', async () => {
  const key = 'context-cancel'
  clearAttachments(key)
  let finish
  const result = takeContext(key, { name: 'Example page', kind: 'document' }, () => new Promise(r => { finish = r }))
  assert.equal(attachments(key)[0].status, 'pending')
  assert.match(outgoing('Read this', attachments(key)).error, /prepar/i)
  cancelImports(key)
  finish({ kind: 'document', text: 'Late text', format: 'Web page' })
  await result
  assert.deepEqual(attachments(key), [])
})

test('failed browser collection remains removable and retryable without losing the draft', async () => {
  const key = 'context-error'
  clearAttachments(key)
  const id = await takeContext(key, { name: 'Example page', kind: 'document' }, async () => { throw new Error('Reload the page and retry.') })
  assert.equal(attachments(key)[0].id, id)
  assert.equal(attachments(key)[0].status, 'error')
  assert.match(attachments(key)[0].error, /Reload/)
  clearAttachments(key)
})
