/**
 * Splash surface for the desktop shell: renders server lifecycle status
 * (`server-status` events) until the Rust side navigates this window to the
 * live dsh web UI. @module splash
 */

import { listen } from '@tauri-apps/api/event'

type ServerStatus =
  | { phase: 'starting'; detail?: string }
  | { phase: 'ready'; url: string }
  | { phase: 'exited'; code: number | null; detail: string }

const statusEl = document.getElementById('status')
const detailEl = document.getElementById('detail')

window.addEventListener('error', (event) => {
  document.title = 'js-error: ' + String(event.message ?? event.error ?? 'unknown')
})

document.title = 'splash'

function setStatus(text: string): void {
  if (statusEl) statusEl.textContent = text
}

function setDetail(text: string): void {
  if (detailEl) detailEl.textContent = text
}

// Reports page state to the shell's status listener (port 32123); the request
// itself is what carries the state, so a CORS-blocked response is fine.
function report(kind: string, detail: string): void {
  const url =
    'http://127.0.0.1:32123/state?kind=' + encodeURIComponent(kind) +
    '&detail=' + encodeURIComponent(detail) +
    '&title=' + encodeURIComponent(document.title) +
    '&href=' + encodeURIComponent(window.location.href)
  fetch(url, { mode: 'no-cors' }).catch(() => {
    /* the status listener is absent; nothing to do */
  })
}

report('loaded', 'splash entry executed')

listen<ServerStatus>('server-status', (event) => {
  const status = event.payload
  switch (status.phase) {
    case 'starting':
      document.title = 'starting'
      setStatus('Starting the harness server…')
      setDetail(status.detail ?? '')
      break
    case 'ready':
      document.title = 'ready'
      setStatus('Server ready — opening DeepSeek Harness…')
      setDetail(status.url)
      window.setTimeout(() => {
        document.title = 'navigating'
        report('navigating', 'setting location.href to ' + status.url)
        try {
          window.location.href = status.url
        } catch (error) {
          document.title = 'nav-error: ' + String(error)
          report('nav-error', String(error))
        }
        window.setTimeout(() => {
          report('after-nav', window.location.href)
        }, 2000)
      }, 250)
      break
    case 'exited': {
      const code = status.code === null ? 'unknown' : String(status.code)
      setStatus('The harness server stopped.')
      setDetail('Exit code ' + code + '. ' + status.detail)
      break
    }
    default:
      break
  }
}).then(() => {
  document.title = 'listening'
  report('listening', 'event bridge registered')
}).catch((error: unknown) => {
  document.title = 'bridge-error'
  report('bridge-error', String(error))
  // The event bridge is absent when the splash is opened outside the shell
  // (e.g. a bare `vite dev`); keep the neutral starting text.
  console.warn('[splash] tauri event bridge unavailable:', error)
})

// A restarting shell relaunches the splash with ?restart=1; keep the
// spinner and show progress instead of an error.
const initialRestart = new URLSearchParams(location.search).get('restart')
if (initialRestart === '1') {
  setStatus('Restarting the harness server…')
  report('restarting', 'server restart in progress')
}

// A crash-recovered shell relaunches the splash with ?error= carrying the
// server's exit detail; render it instead of the spinner.
const initialError = new URLSearchParams(location.search).get('error')
if (initialError) {
  setStatus('The harness server stopped.')
  setDetail(initialError)
  document.querySelector('.splash__spinner')?.remove()
}
