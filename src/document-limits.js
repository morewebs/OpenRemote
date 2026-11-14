export const MAX_DOCUMENT_BYTES = 10 * 1024 * 1024
export const MAX_DOCUMENT_CHARS = 200_000
export const MAX_DRAFT_CHARS = 500_000

export function checkedText(text) {
  if (text.length > MAX_DOCUMENT_CHARS) throw new Error('This document exceeds 200,000 extracted characters. Split it into smaller files.')
  if (!text.trim()) throw new Error('No readable text was found. For a scanned PDF, add photos of its pages instead.')
  return text.trim()
}
