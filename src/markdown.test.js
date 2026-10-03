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
  // The escaped pipe stays one cell with a literal | — backslash
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

test('math renders through KaTeX — inline, display, and dollars stay prose', () => {
  // Inline math with real TeX content.
  const inlineMath = renderMarkdown('The value $e^{i\pi} + 1 = 0$ holds.')
  assert.ok(inlineMath.includes('katex'), 'inline math renders')
  assert.ok(!inlineMath.includes('$e^{'), 'the delimiters are consumed')

  // Display math, the real transcript formula's shape.
  const display = renderMarkdown('$$\n\text{hop}(d) = \arg\max_{p \in \text{table}} |p|\n$$')
  assert.ok(display.includes('md-math-block'), 'display math gets its block')
  assert.ok(display.includes('katex-display'), 'KaTeX display mode')

  // Dollar amounts are never math — no TeX-shaped content between them.
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
