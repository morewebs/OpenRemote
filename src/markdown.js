// Markdown for agent messages - the shapes harnesses actually emit:
// headings, fenced code, tables, nested ordered/unordered lists, task
// lists, blockquotes, strikethrough, inline code/bold/italic, http(s)
// links (titled and bare), images-as-links, horizontal rules, hard breaks,
// and math (inline $…$ and display $$…$$) through KaTeX - the same engine
// GitHub uses, bundled locally, no network calls. Escaped first so nothing
// raw ever renders - agent output is untrusted input; we render only
// shapes we built or KaTeX's own output.
//
// Deliberately not rendered, by design: raw HTML (stays inert),
// footnotes, definition lists, mermaid.

import katex from 'katex'

const ESCAPE = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
}

function escapeHtml(text) {
  return String(text ?? '').replace(/[&<>"]/g, (c) => ESCAPE[c])
}

/// A safe href: http(s) only - javascript:, data:, and everything else
/// never render as links.
function safeHref(url) {
  return /^https?:\/\//i.test(url) ? url : null
}

/// Inline: `code`, **bold**, *italic*, ~~strike~~, [text](url "title"),
/// ![alt](url) (as a link), bare http(s) URLs, $math$. Code spans and
/// math are extracted first so nothing inside them is touched - KaTeX
/// gets the raw source, everything else gets the escaped form.
function inline(text) {
  const codes = []
  const maths = []
  let out = escapeHtml(text)
  // Stash code spans so later passes can't mangle their contents.
  out = out.replace(/`([^`]+)`/g, (_, code) => {
    codes.push(code)
    return `\u0000${codes.length - 1}\u0000`
  })
  // Inline math: $…$ - only when the content actually looks like TeX (a
  // backslash, superscript, subscript, or brace). "$5 and $10" is prose,
  // never math. KaTeX renders the raw source; a parse error keeps the
  // original as plain text.
  out = out.replace(/(?<![\\$])\$([^$\n]*[\\^_{}][^$\n]*)\$(?!\$)/g, (m, tex) => {
    try {
      maths.push(katex.renderToString(tex, { throwOnError: false }))
      return `\u0001${maths.length - 1}\u0001`
    } catch {
      return m
    }
  })
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  out = out.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, '$1<em>$2</em>')
  out = out.replace(/~~([^~]+)~~/g, '<s>$1</s>')
  // Images render as their alt text linked - no inline <img>, the source
  // stays the renderer's own shapes. The title is escaped-quote form by now.
  out = out.replace(
    /!\[([^\]]*)\]\((https?:\/\/[^)\s]+)(?:\s+&quot;[^&]*&quot;)?\)/g,
    '<a class="md-link" href="$2" target="_blank" rel="noopener noreferrer">$1</a>',
  )
  // Links, optional title in the parens - parsed before autolinking so a
  // titled link's URL isn't autolinked inside its own parens.
  out = out.replace(
    /\[([^\]]+)\]\((https?:\/\/[^)\s]+)(?:\s+&quot;[^&]*&quot;)?\)/g,
    '<a class="md-link" href="$2" target="_blank" rel="noopener noreferrer">$1</a>',
  )
  // Bare http(s) URLs autolink - trailing sentence punctuation stays out
  // of the href. Never one already inside a href=".
  out = out.replace(
    /(?<!href=")(https?:\/\/[^\s<]+?)([.,;:!?)\]]*)(?=[\s<]|$)/g,
    '<a class="md-link" href="$1" target="_blank" rel="noopener noreferrer">$1</a>$2',
  )
  // Restore the code spans, escaped, wrapped; then the math.
  out = out.replace(/\u0000(\d+)\u0000/g, (_, i) =>
    `<code class="md-code">${codes[Number(i)]}</code>`,
  )
  out = out.replace(/\u0001(\d+)\u0001/g, (_, i) => maths[Number(i)] ?? '')
  return out
}

/// Split a table row into cells. A backslash-escaped pipe doesn't split.
function splitRow(line) {
  const cells = []
  let cur = ''
  for (let i = 0; i < line.length; i++) {
    const c = line[i]
    if (c === '\\' && line[i + 1] === '|') {
      cur += '|'
      i++
    } else if (c === '|') {
      cells.push(cur)
      cur = ''
    } else {
      cur += c
    }
  }
  cells.push(cur)
  // Strip the flanking pipes if present.
  if (cells.length > 1 && cells[0].trim() === '') cells.shift()
  if (cells.length > 0 && cells[cells.length - 1].trim() === '') cells.pop()
  return cells.map((c) => c.trim())
}

/// The delimiter row of a table: pipes, optional spaces, optional
/// colons (left/center/right alignment markers), at least one dash run.
function isTableDivider(line) {
  const cells = splitRow(line)
  if (cells.length < 1) return false
  return cells.every((c) => /^:?-{2,}:?$/.test(c)) && cells.length > 0
}

const ALIGN = {
  ':-': 'left',
  '-:': 'right',
  ':-:': 'center',
}

function cellAlign(cell) {
  const m = cell.match(/^(?::?)(-{2,})(:?)$/) ?? cell.match(/^(:?)(-{2,})(:?)$/)
  if (!m) return null
  const left = cell.startsWith(':')
  const right = cell.endsWith(':')
  if (left && right) return 'center'
  if (right) return 'right'
  if (left) return 'left'
  return null
}

/// One list item's content and depth: indentation (every 2-3 spaces is a
/// level), ordered-ness, the marker text.
function parseListItem(line) {
  const m = line.match(/^(\s*)([-*+*]|\d+[.)])\s+(.*)$/)
  if (!m) return null
  const depth = Math.floor(m[1].replace(/\t/g, '  ').length / 2)
  const ordered = /\d/.test(m[2])
  const task = m[3].match(/^\[([ xX])\]\s+(.*)$/)
  return {
    depth,
    ordered,
    text: task ? task[2] : m[3],
    checked: task ? task[1] !== ' ' : null,
  }
}

/// Render a markdown string to an HTML fragment. Blank lines separate
/// blocks; a fence left open at the end still shows its code (a streaming
/// reply may be mid-fence).
export function renderMarkdown(text) {
  const src = String(text ?? '')
  const lines = src.split('\n')
  const out = []
  let fence = null
  let buffer = [] // paragraph lines
  let listStack = [] // open list depths, for <ul>/<ol> nesting
  let table = null // { align: [..], rows: [[..]] }
  let displayMath = null // the $$…$$ block's lines, until its closer

  const closeLists = () => {
    while (listStack.length > 0) {
      out.push(listStack.pop() === 'ol' ? '</ol>' : '</ul>')
    }
  }

  const flushParagraph = () => {
    if (buffer.length > 0) {
      // A lone run of 3+ dashes/asterisks mid-paragraph is a rule, not text.
      if (buffer.length === 1 && /^\s*([-*_])(\s*\1){2,}\s*$/.test(buffer[0].raw ?? '')) {
        out.push('<hr class="md-hr" />')
      } else {
        out.push(`<p>${buffer.map((l) => l.html).join('<br />')}</p>`)
      }
      buffer = []
    }
  }

  const flushTable = () => {
    if (table != null) {
      const head = table.rows[0]
      const body = table.rows.slice(1)
      let html = '<div class="md-table-wrap"><table class="md-table"><thead><tr>'
      head.forEach((cell, i) => {
        const a = table.align[i]
        html += `<th${a ? ` style="text-align:${a}"` : ''}>${inline(cell)}</th>`
      })
      html += '</tr></thead>'
      if (body.length > 0) {
        html += '<tbody>'
        body.forEach((row) => {
          html += '<tr>'
          row.forEach((cell, i) => {
            const a = table.align[i]
            html += `<td${a ? ` style="text-align:${a}"` : ''}>${inline(cell)}</td>`
          })
          html += '</tr>'
        })
        html += '</tbody>'
      }
      html += '</table></div>'
      out.push(html)
      table = null
    }
  }

  const flushAll = () => {
    flushParagraph()
    closeLists()
    flushTable()
  }

  for (const line of lines) {
    // ── display math: $$ opens, $$ closes, everything between is TeX ──
    if (displayMath != null) {
      if (/^\s*\$\$\s*$/.test(line)) {
        const tex = displayMath.join('\n')
        try {
          out.push(`<div class="md-math-block">${katex.renderToString(tex, { displayMode: true, throwOnError: false })}</div>`)
        } catch {
          out.push(`<p>${escapeHtml(tex)}</p>`)
        }
        displayMath = null
      } else {
        displayMath.push(line)
      }
      continue
    }
    if (/^\s*\$\$\s*$/.test(line)) {
      flushAll()
      displayMath = []
      continue
    }

    // ── fenced code ──
    const fenceMatch = line.match(/^\s*```(.*)$/)
    if (fenceMatch) {
      if (fence == null) {
        flushAll()
        fence = { lines: [] }
      } else {
        out.push(
          `<pre class="md-pre"><code>${escapeHtml(fence.lines.join('\n'))}</code></pre>`,
        )
        fence = null
      }
      continue
    }
    if (fence != null) {
      fence.lines.push(line)
      continue
    }

    // ── table rows ──
    if (/^\s*\|/.test(line)) {
      const cells = splitRow(line.trim())
      if (table != null) {
        // A second delimiter row after the header is malformed; treat as body.
        table.rows.push(cells)
      } else if (buffer.length > 0 && isTableDivider(line)) {
        // The previous buffered line was the header.
        const header = buffer.pop()
        flushParagraph()
        table = { align: cells.map(cellAlign), rows: [splitRow(header.raw)] }
      } else {
        // A table starts only right after its header line. If we're not in
        // one, hold the line as a paragraph candidate (it may be the header).
        buffer.push({ raw: line.trim(), html: inline(line.trim()) })
      }
      continue
    }
    if (table != null) {
      // A non-pipe line closes the table.
      flushTable()
    }

    // ── blank line ──
    if (line.trim() === '') {
      flushParagraph()
      closeLists()
      continue
    }

    // ── heading ──
    const heading = line.match(/^(#{1,4})\s+(.*)$/)
    if (heading) {
      flushAll()
      const level = heading[1].length
      out.push(`<p class="md-h${level}">${inline(heading[2])}</p>`)
      continue
    }

    // ── horizontal rule ──
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) {
      flushAll()
      out.push('<hr class="md-hr" />')
      continue
    }

    // ── blockquote ── (strip every level of > markers, keep the text)
    const quote = line.match(/^\s*((?:>\s?)+)(.*)$/)
    if (quote) {
      flushAll()
      out.push(`<blockquote class="md-quote">${inline(quote[2])}</blockquote>`)
      continue
    }

    // ── list item ──
    const item = parseListItem(line)
    if (item) {
      flushParagraph()
      const tag = item.ordered ? 'ol' : 'ul'
      // The stack holds one entry per open nesting level; an item at depth
      // d wants the stack exactly d+1 deep (0-indexed depth → 1-indexed
      // stack). Close deeper lists, or a same-depth list of the other kind.
      while (listStack.length > item.depth + 1) {
        out.push(listStack.pop() === 'ol' ? '</ol>' : '</ul>')
      }
      if (listStack.length === item.depth + 1 && listStack[item.depth] !== tag) {
        out.push(listStack.pop() === 'ol' ? '</ol>' : '</ul>')
      }
      while (listStack.length < item.depth + 1) {
        out.push(`<${tag}>`)
        listStack.push(tag)
      }
      const checkbox = item.checked != null
        ? `<span class="md-task${item.checked ? ' done' : ''}">${item.checked ? '✓' : '○'}</span>`
        : ''
      out.push(`<li>${checkbox}${inline(item.text)}</li>`)
      continue
    }

    // ── plain text: a paragraph line (may be a table header next) ──
    // A trailing backslash is a hard break; the join supplies the <br />.
    const raw = line.trim().replace(/\\$/, '')
    buffer.push({ raw, html: inline(raw) })
  }

  // A stream that ends mid-fence still shows its code so far; mid-$$ its
  // math so far (a stream may be cut inside a formula).
  if (fence != null) {
    out.push(`<pre class="md-pre"><code>${escapeHtml(fence.lines.join('\n'))}</code></pre>`)
  }
  if (displayMath != null && displayMath.length > 0) {
    const tex = displayMath.join('\n')
    try {
      out.push(`<div class="md-math-block">${katex.renderToString(tex, { displayMode: true, throwOnError: false })}</div>`)
    } catch {
      out.push(`<p>${escapeHtml(tex)}</p>`)
    }
  }
  flushAll()
  return out.join('')
}
