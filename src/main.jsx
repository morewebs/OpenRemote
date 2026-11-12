import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/geist-mono/400.css'
import 'katex/dist/katex.min.css'
import './privacy.css'
import App from './App.jsx'
import Splash from './Splash.jsx'
import { restoreBootPrefs } from './settings.js'
import { openLinksOutside } from './platform.js'
import { startUpdateChecks } from './updates.js'

// Before first paint: the persisted reduce-motion, density and privacy
// mode, so the app never flashes animated, mis-spaced or readable on a
// restart.
restoreBootPrefs(document, window.location)
openLinksOutside(document)
// The once-a-day look for a newer signed release (desktop only; the first
// look waits until the app has come up).
startUpdateChecks()

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <App />
    <Splash />
  </StrictMode>,
)
