const PALETTE = ['slate', 'teal', 'indigo', 'amber', 'rose', 'emerald']
const utf8 = new TextEncoder()
const segmenter = typeof Intl.Segmenter === 'function' ? new Intl.Segmenter('und', { granularity: 'grapheme' }) : null

function firstGrapheme(text) {
  return segmenter?.segment(text).containing(0)?.segment ?? text.match(/^\P{Mark}\p{Mark}*/u)?.[0] ?? ''
}

export function avatarInitials(title) {
  const words = (typeof title === 'string' ? title : '').normalize('NFC').match(/[\p{Letter}\p{Number}][\p{Letter}\p{Number}\p{Mark}]*/gu) ?? []
  if (words.length === 0) return '?'
  const seeds = words.length === 1 ? words : [words[0], words.at(-1)]
  // An uppercase mapping can expand (such as sharp S). Keep one grapheme
  // from each word so a long mapping cannot crowd out the second initial.
  return seeds.map((word) => firstGrapheme(firstGrapheme(word).toUpperCase())).join('')
}

export function defaultAgentAvatar(id, title) {
  // FNV-1a on the id, never the name, so a rename keeps the same color.
  let hash = 0x811c9dc5
  for (const byte of utf8.encode(id ?? '')) hash = Math.imul(hash ^ byte, 0x01000193)
  return { initials: avatarInitials(title), color: PALETTE[(hash >>> 0) % PALETTE.length] }
}
