// Readable reference content, without executing macros, following relationships
// outside an archive, or turning document markup into live HTML.
import { unzipSync, strFromU8 } from 'fflate'
import { XMLParser, XMLValidator } from 'fast-xml-parser'

export const MAX_DOCUMENT_BYTES = 10 * 1024 * 1024
export const MAX_DOCUMENT_CHARS = 200_000
export const MAX_DRAFT_CHARS = 500_000
const TEXT_EXT = new Set('txt md markdown csv tsv json jsonl log yaml yml xml html htm css scss js jsx mjs cjs ts tsx py rs go c h cpp hpp cs java kt swift rb php sh bash ps1 sql toml ini cfg conf env tex rst diff patch ipynb svelte vue graphql'.split(' '))

export function documentType(name = '', type = '') {
  const ext = name.split('.').pop().toLowerCase()
  if (['pdf', 'docx', 'xlsx'].includes(ext)) return ext
  if (['doc', 'xls', 'ppt', 'pptx', 'exe', 'dll', 'zip', 'bin'].includes(ext)) return null
  return TEXT_EXT.has(ext) || /^text\//.test(type) || ['application/json', 'application/xml'].includes(type) ? 'text' : null
}

export function checkedText(text) {
  if (text.length > MAX_DOCUMENT_CHARS) throw new Error('This document exceeds 200,000 extracted characters. Split it into smaller files.')
  if (!text.trim()) throw new Error('No readable text was found. For a scanned PDF, add photos of its pages instead.')
  return text.trim()
}

const xmlParser = new XMLParser({ preserveOrder: true, ignoreAttributes: false, removeNSPrefix: true, parseTagValue: false, trimValues: false, processEntities: false })
const decode = (s) => String(s).replace(/&(#x[\da-f]+|#\d+|amp|lt|gt|quot|apos);/gi, (_, e) => {
  const known = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" }
  if (known[e]) return known[e]
  const n = e.startsWith('#x') ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10)
  return n > 0 && n <= 0x10ffff ? String.fromCodePoint(n) : ''
})

function xml(bytes) {
  const text = strFromU8(bytes)
  if (/<!DOCTYPE|<!ENTITY/i.test(text)) throw new Error('XML declarations are not supported in document imports.')
  if (XMLValidator.validate(text) !== true) throw new Error('This document contains invalid XML. Export it again and retry.')
  return xmlParser.parse(text)
}

function nodes(tree, tag) {
  const out = []
  for (const node of tree ?? []) {
    if (node[tag]) out.push(node)
    for (const [key, value] of Object.entries(node)) if (key !== ':@' && Array.isArray(value)) out.push(...nodes(value, tag))
  }
  return out
}

function words(tree, word = false) {
  let text = ''
  for (const node of tree ?? []) {
    for (const [key, value] of Object.entries(node)) {
      if (key === '#text') text += decode(value)
      else if (key === 'tab') text += '\t'
      else if (key === 'br') text += '\n'
      else if (Array.isArray(value)) {
        text += words(value, word)
        if (word && ['p', 'tr'].includes(key)) text += '\n'
        if (word && key === 'tc') text += '\t'
      }
      if (text.length > MAX_DOCUMENT_CHARS) throw new Error('This document exceeds 200,000 extracted characters. Split it into smaller files.')
    }
  }
  return text
}

function officeEntries(bytes) {
  let total = 0
  try {
    return unzipSync(bytes, { filter: entry => {
      total += entry.originalSize
      if (entry.originalSize > 20 * 1024 * 1024 || total > 40 * 1024 * 1024) throw new Error('This document expands too large. Export a smaller document and retry.')
      return /^(word\/document\.xml|xl\/(workbook\.xml|_rels\/workbook\.xml\.rels|sharedStrings\.xml|worksheets\/sheet[^/]*\.xml))$/.test(entry.name)
    } })
  } catch (err) {
    if (err.message.includes('expands')) throw err
    throw new Error('This document could not be read. It may be encrypted or corrupt; export it again and retry.')
  }
}

function spreadsheet(entries) {
  if (!entries['xl/workbook.xml'] || !entries['xl/_rels/workbook.xml.rels']) throw new Error('This spreadsheet is missing its workbook. Export it again and retry.')
  const sheets = nodes(xml(entries['xl/workbook.xml']), 'sheet')
  const rels = nodes(xml(entries['xl/_rels/workbook.xml.rels']), 'Relationship')
  const strings = entries['xl/sharedStrings.xml'] ? nodes(xml(entries['xl/sharedStrings.xml']), 'si').map(n => words(nodes(n.si, 't').flatMap(t => t.t))) : []
  const blocks = []
  for (const sheet of sheets) {
    const attr = sheet[':@'] ?? {}
    const rel = rels.find(n => n[':@']?.['@_Id'] === attr['@_id'])?.[':@']
    const path = rel?.['@_Target']?.replace(/^\/?xl\//, '').replace(/^\//, '')
    if (!path || path.includes('..') || rel?.['@_TargetMode'] === 'External' || !entries[`xl/${path}`]) throw new Error('A spreadsheet sheet could not be read. Export it again and retry.')
    const lines = []
    for (const row of nodes(xml(entries[`xl/${path}`]), 'row')) {
      const cells = []
      for (const cell of nodes(row.row, 'c')) {
        const a = cell[':@'] ?? {}
        const col = a['@_r']?.match(/^[A-Z]+/)?.[0] ?? 'A'
        const index = [...col].reduce((n, c) => n * 26 + c.charCodeAt(0) - 64, 0) - 1
        if (index > 16383) throw new Error('This spreadsheet has an invalid column.')
        while (cells.length <= index) cells.push('')
        const value = words(nodes(cell.c, a['@_t'] === 'inlineStr' ? 't' : 'v').flatMap(n => n.t ?? n.v))
        cells[index] = a['@_t'] === 's' ? strings[Number(value)] ?? '' : a['@_t'] === 'b' ? (value === '1' ? 'TRUE' : 'FALSE') : value
      }
      lines.push(cells.join('\t'))
      checkedLength(lines.join('\n'))
    }
    blocks.push(`Sheet: ${decode(attr['@_name'] ?? 'Sheet')}\n${lines.join('\n')}`)
    checkedLength(blocks.join('\n\n'))
  }
  if (!blocks.length || !blocks.some(b => b.slice(b.indexOf('\n') + 1).trim())) throw new Error('No readable text was found in this spreadsheet.')
  return blocks.join('\n\n')
}

function checkedLength(text) {
  if (text.length > MAX_DOCUMENT_CHARS) checkedText(text)
}

export async function parseDocumentBytes({ name, type = '', bytes }) {
  if (bytes.byteLength > MAX_DOCUMENT_BYTES) throw new Error('This file is over 10 MB. Choose a smaller document.')
  const kind = documentType(name, type)
  if (!kind) throw new Error('This file type is not supported. Use PDF, DOCX, XLSX or a UTF-8 text file.')
  let text
  if (kind === 'text') {
    try { text = new TextDecoder('utf-8', { fatal: true }).decode(bytes) } catch { throw new Error('This file is not UTF-8 text. Export it as UTF-8 and retry.') }
    if (/[\x00-\x08\x0e-\x1f]/.test(text)) throw new Error('This file contains binary data. Choose a UTF-8 text document.')
  } else if (kind === 'pdf') {
    const { pdfText } = await import('./document-pdf.js')
    text = await pdfText(bytes)
  } else {
    const entries = officeEntries(bytes)
    if (kind === 'docx') {
      if (!entries['word/document.xml']) throw new Error('This Word document could not be read. Export it again and retry.')
      text = words(xml(entries['word/document.xml']), true)
    } else text = spreadsheet(entries)
  }
  return { text: checkedText(text), format: { text: 'Text', pdf: 'PDF', docx: 'Word', xlsx: 'Spreadsheet' }[kind] }
}
