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
  assert.match(html, /<span class="md-li">one<\/span>/)
  assert.ok(html.includes('Done.'))
})
