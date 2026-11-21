// Text that arrives as UTF-8 read through a single-byte Windows code page
// ("don’t" shown as three odd characters) is repaired for display. Windows
// PowerShell 5.1 does this to every UTF-8 file it prints without a byte
// order mark, so a command that read one hands the chat garbled words that
// no renderer can fix, only this can. Pure and strict: a sequence is
// repaired only when its characters, taken back to windows-1252 bytes, are
// one valid UTF-8 character. Anything that decodes any other way stays.

// What a windows-1252 decoder shows for bytes 0x80 to 0x9F. The five bytes
// it leaves undefined show as the control character of the same number.
const CP1252_HIGH = [
  0x20ac, 0x81, 0x201a, 0x0192, 0x201e, 0x2026, 0x2020, 0x2021, 0x02c6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8d, 0x017d, 0x8f,
  0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x02dc, 0x2122, 0x0161, 0x203a, 0x0153, 0x9d, 0x017e, 0x0178,
]
const BYTE_OF = new Map(CP1252_HIGH.map((code, i) => [code, 0x80 + i]))

// A lead byte (0xC2 to 0xF4) followed by what a continuation byte becomes:
// the cheap test that lets clean text, nearly all of it, skip the work.
const SIGNATURE = /[\u00c2-\u00f4][\u0080-\u00bf\u0152\u0153\u0160\u0161\u0178\u017d\u017e\u0192\u02c6\u02dc\u2013\u2014\u2018-\u201e\u2020-\u2022\u2026\u2030\u2039\u203a\u20ac\u2122]/

// The byte a character stands for in a windows-1252 reading, or -1.
function byteOf(ch) {
  const code = ch.codePointAt(0)
  if (code < 0x80) return code
  if (code >= 0x80 && code <= 0xff) return code
  return BYTE_OF.get(code) ?? -1
}

const decoder = new TextDecoder('utf-8', { fatal: true })

/**
 * `{ text, repaired }`: `text` with every UTF-8 sequence that was read as
 * windows-1252 put back, `repaired` true when anything changed.
 */
export function repairMojibake(input) {
  if (typeof input !== 'string' || !SIGNATURE.test(input)) return { text: input, repaired: false }
  const chars = Array.from(input)
  let out = ''
  let repaired = false
  for (let i = 0; i < chars.length; i++) {
    const lead = byteOf(chars[i])
    const length = lead >= 0xf0 && lead <= 0xf4 ? 4 : lead >= 0xe0 ? 3 : lead >= 0xc2 ? 2 : 1
    if (lead >= 0xc2 && lead <= 0xf4 && i + length <= chars.length) {
      const bytes = [lead]
      for (let j = 1; j < length; j++) bytes.push(byteOf(chars[i + j]))
      if (bytes.every((b, n) => n === 0 || (b >= 0x80 && b <= 0xbf))) {
        try {
          out += decoder.decode(Uint8Array.from(bytes))
          repaired = true
          i += length - 1
          continue
        } catch {
          // not one character: leave what was there
        }
      }
    }
    out += chars[i]
  }
  return { text: repaired ? out : input, repaired }
}
