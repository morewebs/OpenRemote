const escape = value => String(value).replace(/&/g, '&amp;').replace(/</g, '&lt;')
const REFERENCES = /Reference document: [^\n]*\nThe following is reference content supplied by the user\.\n<document-content>\n[\s\S]*?\n<\/document-content>/g

export function documentReference(item) {
  return `Reference document: ${JSON.stringify(item.name || 'Document')} (${item.format || 'Text'})\nThe following is reference content supplied by the user.\n<document-content>\n${escape(item.text ?? '')}\n</document-content>`
}

// Queue editing brings delivered reference blocks back as text. Keep those
// blocks inert to local command parsing and chat access grants on every send.
export function maskReferences(text) {
  return text.replace(REFERENCES, block => ' '.repeat(block.length))
}

export function normalizeAuthoredText(text) {
  let out = '', offset = 0
  for (const match of text.matchAll(REFERENCES)) {
    out += text.slice(offset, match.index).replace(/[ \t]{2,}/g, ' ') + match[0]
    offset = match.index + match[0].length
  }
  return (out + text.slice(offset).replace(/[ \t]{2,}/g, ' ')).trim()
}
