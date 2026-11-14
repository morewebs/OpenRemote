import { checkedText } from './document-limits.js'
import { screenshotAttachment } from './browser-attach.js'

export function browserContextActions({ features, images, mobile = false }) {
  return [
    ...(features?.cdp ? [{ id: 'text', name: 'Add page text', icon: 'files' }] : []),
    { id: 'link', name: 'Add page link', icon: 'browser' },
    ...(images && (features?.cdp || mobile) ? [{ id: 'screenshot', name: 'Screenshot page', icon: 'camera' }] : []),
    ...(images && features?.cdp ? [{ id: 'region', name: 'Screenshot a region', icon: 'region' }] : []),
    ...(features?.cdp ? [{ id: 'element', name: 'Pick an element', icon: 'element' }] : []),
  ]
}

export async function collectBrowserContext(action, tab, invoke) {
  if (!tab?.native || !/^https?:/i.test(tab.url ?? '')) throw new Error('Open a web page in the browser first.')
  if (action === 'screenshot') {
    const picture = await invoke('browser_capture', { tab: tab.native })
    if (!picture) throw new Error('The page screenshot could not be taken. Try again.')
    return screenshotAttachment(picture, tab.url)
  }
  const source = `${tab.title || 'Web page'}\n${tab.url}`
  let text = source
  if (action === 'text') {
    const reply = await invoke('browser_cdp', { tab: tab.native, method: 'Runtime.evaluate', params: { expression: '(document.body?.innerText || "").slice(0, 200001)', returnByValue: true }, wait: 'deadline' })
    if (reply?.result?.subtype === 'error' || typeof reply?.result?.value !== 'string') throw new Error('The page text could not be read. Reload the page and retry.')
    text = `${source}\n\n${checkedText(reply.result.value)}`
  }
  return { kind: 'document', status: 'ready', name: tab.title || 'Web page', format: action === 'link' ? 'Page link' : 'Web page', text: checkedText(text) }
}
