// Minimal markdown for agent messages — code fences, inline code, bold,
// italics, links, and headings, escaped first so nothing raw ever renders.
// No dependencies: agent output is untrusted input; we render only shapes we
// built, in the design contract's own voice (Inter for prose, Geist Mono for
// technical content, one accent, no new vocabulary).

const ESCAPE = {
  '&': '&amp;',
  '<': '&lt;',
  '>': '&gt;',
  '"': '&quot;',
}

function escapeHtml(text) {
  return String(text ?? '').replace(/[&<>"]/g, (c) => ESCAPE[c])
}

/// Inline: `code`, **bold**, *italic*, [text](url). URLs only http(s) —
/// javascript: and data: never render as links.
function inline(text) {
  let out = escapeHtml(text)
  out = out.replace(/`([^`]+)`/g, '<code class="md-code">$1</code>')
  out = out.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
  out = out.replace(/(^|[^*])\*([^*]+)\*(?!\*)/g, '$1<em>$2</em>')
  out = out.replace(
    /\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g,
    '<a class="md-link" href="$2" target="_blank" rel="noopener noreferrer">$1</a>',
  )
  return out
}

/// Render a markdown-ish string to an HTML fragment. Blank lines separate
/// paragraphs; fenced blocks become <pre><code>; ``` closes an unclosed
/// fence at the end (a streaming reply may still be mid-fence).
export function renderMarkdown(text) {
  const src = String(text ?? '')
  const lines = src.split('\n')
  const out = []
  let fence = null
  let buffer = []

  const flushParagraph = () => {
    if (buffer.length > 0) {
      out.push(`<p>${buffer.join('<br />')}</p>`)
      buffer = []
    }
  }

  for (const line of lines) {
    const fenceMatch = line.match(/^\s*```(.*)$/)
    if (fenceMatch) {
      if (fence == null) {
        flushParagraph()
        fence = { lang: fenceMatch[1].trim() || null, lines: [] }
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
    if (line.trim() === '') {
      flushParagraph()
      continue
    }
    const heading = line.match(/^(#{1,4})\s+(.*)$/)
    if (heading) {
      flushParagraph()
      const level = heading[1].length
      out.push(`<p class="md-h${level}">${inline(heading[2])}</p>`)
      continue
    }
    const bullet = line.match(/^\s*[-*]\s+(.*)$/)
    if (bullet) {
      buffer.push(`<span class="md-li">${inline(bullet[1])}</span>`)
      continue
    }
    buffer.push(inline(line))
  }
  // A stream that ends mid-fence still shows its code so far.
  if (fence != null) {
    out.push(`<pre class="md-pre"><code>${escapeHtml(fence.lines.join('\n'))}</code></pre>`)
  }
  flushParagraph()
  return out.join('')
}
