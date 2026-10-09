import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource/inter/400.css'
import '@fontsource/inter/500.css'
import '@fontsource/inter/600.css'
import '@fontsource/geist-mono/400.css'
import 'katex/dist/katex.min.css'
import App from './App.jsx'
import { restoreBootPrefs } from './settings.js'
import { openLinksOutside } from './platform.js'

// Before first paint: the persisted reduce-motion and density, so the
// app never flashes animated or mis-spaced on a restart.
restoreBootPrefs(document, window.location)
openLinksOutside(document)

createRoot(document.getElementById('root')).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
