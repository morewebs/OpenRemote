import assert from 'node:assert/strict'
import test from 'node:test'
import { renderMarkdown } from './markdown.js'

test('prose paragraphs render escaped, blank lines split them', () => {
  const html = renderMarkdown('Compare <div> & "quotes" please.\n\nSecond paragraph.')
  assert.match(html, /Compare &lt;div&gt; &amp; &quot;quotes&quot; please\./)
  assert.ok(html.includes('Second paragraph.'))
  assert.ok(!html.includes('<div>'), 'raw html never survives')
})

test('fenced code blocks render as pre, content escaped, mid-fence streams too', () => {
  const html = renderMarkdown('Before.\n```rust\nlet x = 1 < 2;\n```\nAfter.')
  assert.ok(html.includes('<pre class="md-pre"><code>let x = 1 &lt; 2;</code></pre>'), 'escaped code inside pre')
  assert.ok(html.includes('Before.') && html.includes('After.'))

  // A stream cut off inside the fence still shows its code.
  const cut = renderMarkdown('Working:\n```js\nconst a = 1')
  assert.match(cut, /<pre class="md-pre"><code>const a = 1<\/code><\/pre>/)
})

test('inline shapes: code, bold, italic, safe links only', () => {
  const html = renderMarkdown('Use `npm test` for **all** the *suites*.')
  assert.match(html, /<code class="md-code">npm test<\/code>/)
  assert.match(html, /<strong>all<\/strong>/)
  assert.match(html, /<em>suites<\/em>/)

  const linked = renderMarkdown('See [docs](https://example.com) now.')
  assert.match(linked, /<a class="md-link" href="https:\/\/example\.com"/)

  // javascript: and data: URLs never become links.
  const hostile = renderMarkdown('See [evil](javascript:alert(1)) now.')
  assert.ok(!hostile.includes('href="javascript:'), 'script URLs stay inert')
  assert.ok(hostile.includes('javascript:alert(1)'), 'the text itself survives, escaped')
})

test('headings and bullets render in the paragraph family', () => {
  const html = renderMarkdown('# Title\n\n- one\n- two\n\nDone.')
  assert.match(html, /<p class="md-h1">Title<\/p>/)
  assert.match(html, /<ul><li>one<\/li><li>two<\/li><\/ul>/)
  assert.ok(html.includes('Done.'))
})

test('a UTF-8 BOM on the first line does not swallow the heading it opens', () => {
  // A harness echoing a file passes along whatever the editor wrote, and
  // Obsidian and Windows editors open a file with a byte order mark. The
  // mark, not the hash, then sits at column zero.
  const html = renderMarkdown('\uFEFF# White Coffee Shop Website Design\n\n## Intent\n\nCopy.')
  assert.match(html, /<p class="md-h1">White Coffee Shop Website Design<\/p>/)
  assert.match(html, /<p class="md-h2">Intent<\/p>/)
  assert.ok(!html.includes('\uFEFF'), 'the mark does not survive into the text')
  assert.ok(!html.includes('# White Coffee'), 'no literal hash is left on the title')
})

test('tables render with alignment and escaped pipes survive', () => {
  const html = renderMarkdown(
    '| Syntax | Notes | Priority |\n' +
    '|:-------|:-----:|---------:|\n' +
    '| `code` | ✅ | 1 |\n' +
    '| a \\| b | works\\|here | 2 |',
  )
  assert.match(html, /<table class="md-table">/)
  assert.match(html, /<th style="text-align:left">Syntax<\/th>/)
  assert.match(html, /<th style="text-align:center">Notes<\/th>/)
  assert.match(html, /<th style="text-align:right">Priority<\/th>/)
  assert.ok(html.includes('<code class="md-code">code</code>'), 'inline code in cells')
  // The escaped pipe stays one cell with a literal | - backslash
  // consumed, cell not split.
  assert.match(html, /<td[^>]*>a \| b<\/td>/)
  assert.match(html, /<td[^>]*>works\|here<\/td>/)
  // A stray pipe line with no header above stays paragraph text.
  const stray = renderMarkdown('nothing above\n| not | a | table |')
  assert.ok(!stray.includes('<table'), 'no header + delimiter = not a table')
})

test('nested lists nest, ordered and unordered mix, tasks check', () => {
  const html = renderMarkdown(
    '1. Ordered parent\n' +
    '   - [ ] Unchecked task\n' +
    '   - [x] Checked task\n' +
    '     1. Deep ordered item\n' +
    '2. Second ordered item\n',
  )
  assert.match(html, /<ol><li>Ordered parent<\/li>/)
  // The unordered sublist opens inside the ordered item's scope.
  assert.match(html, /<ul><li><span class="md-task">○<\/span>Unchecked task<\/li>/)
  assert.match(html, /<li><span class="md-task done">✓<\/span>Checked task<\/li>/)
  assert.match(html, /<ol><li>Deep ordered item<\/li><\/ol>/)
  assert.match(html, /<li>Second ordered item<\/li><\/ol>/)
})

test('strikethrough, titled links, bare URLs, images-as-links, rules, blockquotes', () => {
  const html = renderMarkdown(
    '~~struck~~ and [titled](https://example.com "hover") and https://example.com bare\n\n' +
    '![alt text](https://example.com/cat.png)\n\n---\n\n> a quoted line',
  )
  assert.match(html, /<s>struck<\/s>/)
  assert.match(html, /<a class="md-link" href="https:\/\/example\.com" target="_blank" rel="noopener noreferrer">titled<\/a>/)
  assert.match(html, />https:\/\/example\.com<\/a>/, 'bare URL autolinks')
  assert.match(html, /<a[^>]*>alt text<\/a>/, 'an image renders as its alt text linked')
  assert.match(html, /<hr class="md-hr" \/>/)
  assert.match(html, /<blockquote class="md-quote">a quoted line<\/blockquote>/)
})

test('javascript: and data: URLs never link, code spans protect their contents', () => {
  const hostile = renderMarkdown('[evil](javascript:alert(1)) and [data](data:text/plain,x)')
  assert.ok(!hostile.includes('href="javascript:'), 'script URLs stay inert')
  assert.ok(!hostile.includes('href="data:'), 'data URLs stay inert')

  const code = renderMarkdown('`**not bold** and https://example.com`')
  assert.match(code, /<code class="md-code">\*\*not bold\*\* and https:\/\/example\.com<\/code>/, 'code spans untouched')
})

test('every href the renderer emits starts with http(s)://', () => {
  // The whole link zoo in one input: if any shape ever emits a non-web
  // href (a widened regex, a new scheme, an img path), this fails - the
  // http(s)-only rule lives in safeHref, on the way out.
  const html = renderMarkdown(
    '[link](https://a.example/x) and ![img](https://b.example/i.png "hover") ' +
    'and https://c.example/auto bare and [evil](javascript:alert(1)) ' +
    'and [data](data:text/plain,x) and ![bad](data:image/svg+xml,1) ' +
    'and [proto-rel](//example.com) too',
  )
  const hrefs = [...html.matchAll(/href="([^"]*)"/g)].map((m) => m[1])
  assert.ok(hrefs.length >= 3, 'the renderer did emit links')
  for (const href of hrefs) {
    assert.match(href, /^https?:\/\//, `non-web href emitted: ${href}`)
  }
})

test('math renders through KaTeX - inline, display, and dollars stay prose', () => {
  // Inline math with real TeX content.
  const inlineMath = renderMarkdown('The value $e^{i\pi} + 1 = 0$ holds.')
  assert.ok(inlineMath.includes('katex'), 'inline math renders')
  assert.ok(!inlineMath.includes('$e^{'), 'the delimiters are consumed')

  // Display math, the real transcript formula's shape.
  const display = renderMarkdown('$$\n\text{hop}(d) = \arg\max_{p \in \text{table}} |p|\n$$')
  assert.ok(display.includes('md-math-block'), 'display math gets its block')
  assert.ok(display.includes('katex-display'), 'KaTeX display mode')

  // Dollar amounts are never math - no TeX-shaped content between them.
  const price = renderMarkdown('That costs $5 and $10 total.')
  assert.ok(!price.includes('katex'), 'prices stay prose')
  assert.ok(price.includes('$5 and $10'), 'the dollars survive')

  // A mid-formula stream cut still shows its math so far.
  const cut = renderMarkdown('$$\nx = \frac{1}{2}')
  assert.ok(cut.includes('katex'), 'an unclosed $$ still renders its content')

  // Code spans protect their dollars from the math pass.
  const code = renderMarkdown('`$not math$`')
  assert.ok(!code.includes('katex'), 'code spans beat math')
})

test('shell code in prose is not typeset as math', () => {
  // Two shell variables on one line used to read as one formula, because the
  // text between the dollars held an underscore.
  const powershell = renderMarkdown("Where-Object { $_.IPAddress -notlike '169.254.*' -and $_.AddressState -eq 'Preferred' }")
  assert.ok(!powershell.includes('katex'), 'two $_ are not a formula')
  assert.ok(powershell.includes('$_.IPAddress'), 'the words are all there')
  const bash = renderMarkdown('NET="${ME%.*}.$(( ${ME##*.} / 4 * 4 ))"')
  assert.ok(!bash.includes('katex'), '${...} is shell')
  // Real formulas still render, including with a subscript.
  assert.ok(renderMarkdown('so $a_i + b_i$ holds').includes('katex'))
  assert.ok(renderMarkdown('so $\\frac{1}{2}$ holds').includes('katex'))
  // The opening dollar needs a non-space after it, the closing one a non-space before it.
  assert.ok(!renderMarkdown('so $ a_i + b_i $ holds').includes('katex'))
})

test('a link to a file reads as its label with the path on hover, never as raw markdown', () => {
  const html = renderMarkdown('See [Incident record and rollback](<C:/Users/me/My Docs/472-peak-traffic.md>) for the record.')
  assert.ok(!html.includes('[Incident'), 'the markdown syntax is consumed')
  assert.ok(!html.includes('](')) 
  assert.ok(html.includes('>Incident record and rollback</span>'), 'the label reads in place')
  assert.ok(html.includes('title="C:/Users/me/My Docs/472-peak-traffic.md"'), 'the path is on hover')
  assert.ok(!html.includes('href='), 'a local path is never a link')
  // A bare relative path, with a title, the same way.
  const rel = renderMarkdown('[the spec](docs/spec.md "Open it")')
  assert.ok(rel.includes('>the spec</span>') && !rel.includes('href='))
})

test('web links still link, with or without angle brackets, and nothing unsafe becomes one', () => {
  assert.ok(renderMarkdown('[site](https://example.com/a)').includes('href="https://example.com/a"'))
  assert.ok(renderMarkdown('[site](<https://example.com/a b>)').includes('href="https://example.com/a b"'))
  // Any other scheme stays the text it was, in plain view, and never links.
  for (const hostile of ['[click](javascript:alert(1))', '[click](data:text/html,x)', '[click](file:///C:/x.md)']) {
    const out = renderMarkdown(hostile)
    assert.ok(!out.includes('href='), `${hostile} is never a link`)
    assert.ok(!out.includes('md-ref'), `${hostile} is not read as a path`)
    assert.ok(out.includes('[click]('), `${hostile} stays visible`)
  }
  // An image keeps its own rule: no stray "!" in front of a label.
  assert.ok(!renderMarkdown('![shot](docs/shot.png)').includes('class="md-ref"'))
})
