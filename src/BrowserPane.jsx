// The browser as a right side-pane: the same BrowserView the app used to
// show whole-window, docked beside the chat. The tabs live in the browser
// store, so closing the pane hides the pages instead of closing them -
// reopening shows them where they were.

import { Suspense, lazy } from 'react'

// Loaded on first opening: the browser costs nothing until it is shown.
const BrowserView = lazy(() => import('./BrowserView.jsx'))
const BrowserPhone = lazy(() => import('./BrowserPhone.jsx'))

// The phone's browser: full screen over the app (BrowserPhone.jsx), the
// same store and pages as the desktop's pane. Android's back leaves it.
export function BrowserPhoneView({ onClose, askToShow = null }) {
  return (
    <Suspense fallback={null}>
      <BrowserPhone onClose={onClose} askToShow={askToShow} />
    </Suspense>
  )
}

export default function BrowserPane({ onClose, askToShow = null, width = null }) {
  return (
    <aside className="bw-pane" aria-label="Browser" style={width ? { flexBasis: `${width}px` } : undefined}>
      <Suspense fallback={null}>
        <BrowserView onClosePane={onClose} askToShow={askToShow} />
      </Suspense>
    </aside>
  )
}
