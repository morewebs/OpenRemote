// Dictation for OpenRemote: speak, and the words land in the composer.
//
// A microphone button in every composer's foot. Click it and it listens -
// the button breathes, with the time beside it; click again and the
// recording goes to the transcription service set in Settings > Plugins >
// Dictation (any OpenAI-compatible /audio/transcriptions endpoint: OpenAI's
// own, another provider's, or a whisper server on this computer), and the
// text goes in at the cursor of the composer you started in. Nothing is
// sent to a chat: you read it, change it, and send it yourself.
//
// OpenRemote reaches the internet for a plugin. A server on this computer
// or your network is reached only at an address you typed into Service
// address yourself: the default never counts, and this code can't choose
// one.
//
// It runs in OpenRemote's sandboxed plugin frame with two powers, the
// microphone and the network. The request goes out through OpenRemote,
// which writes the API key in on the way, and only toward the service
// address: this code never holds the key.

const INPUT = 'dictate'

// Shorter than this is a click, not speech: nothing is sent.
const SHORTEST_MS = 700

// Opus at 32 kbps keeps a minute of speech near 240 KB, well inside every
// service's upload limit, and transcribes as well as anything larger.
const TYPES = ['audio/webm;codecs=opus', 'audio/webm', 'audio/ogg;codecs=opus', 'audio/mp4']
const BITS_PER_SECOND = 32_000

// The recording under way, or null; whether one is starting (the system
// may be asking about the microphone); and whether one is being written
// down.
let recording = null
let starting = false
let transcribing = false

openremote.composer.onClick(({ input, composer }) => {
  if (input !== INPUT) return
  // While the microphone opens, or the words are on their way, a click
  // waits for them.
  if (starting || transcribing) return
  if (recording) stop()
  else {
    starting = true
    start(composer).finally(() => {
      starting = false
    })
  }
})

function show(state, label, title, composer) {
  openremote.composer.setState(INPUT, { state, label, title }, { composer }).catch(() => {})
}

const clock = (ms) => {
  const seconds = Math.floor(ms / 1000)
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`
}

const clampMinutes = (value) => Math.min(30, Math.max(1, Number.isFinite(Number(value)) && Number(value) > 0 ? Number(value) : 10))

function microphoneWords(err) {
  switch (err?.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return "Dictation may not use the microphone. Allow it when asked, or in your system's microphone privacy settings."
    case 'NotFoundError':
    case 'OverconstrainedError':
      return 'No microphone was found.'
    case 'NotReadableError':
    case 'AbortError':
      return 'The microphone is busy or unavailable. Another app may be using it.'
    default:
      return `The microphone did not start: ${err?.message ?? err}`
  }
}

function release(stream) {
  for (const track of stream?.getTracks() ?? []) track.stop()
}

async function start(composer) {
  let settings
  try {
    settings = await openremote.settings.get()
  } catch (err) {
    openremote.ui.toast(`Dictation could not read its settings: ${err.message}`)
    return
  }
  if (!String(settings.values.base_url ?? '').trim()) {
    openremote.ui.toast('Dictation needs a transcription service first: set its address in Settings.')
    openremote.ui.openSettings().catch(() => {})
    return
  }
  let stream
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    })
  } catch (err) {
    openremote.ui.toast(microphoneWords(err))
    return
  }
  if (typeof MediaRecorder === 'undefined') {
    release(stream)
    openremote.ui.toast("This system can't record audio here.")
    return
  }
  const type = TYPES.find((t) => MediaRecorder.isTypeSupported?.(t)) ?? ''
  let recorder
  try {
    recorder = new MediaRecorder(stream, type ? { mimeType: type, audioBitsPerSecond: BITS_PER_SECOND } : { audioBitsPerSecond: BITS_PER_SECOND })
  } catch (err) {
    release(stream)
    openremote.ui.toast(`The recording did not start: ${err.message}`)
    return
  }
  const minutes = clampMinutes(settings.values.max_minutes)
  const state = { recorder, stream, chunks: [], composer, settings, startedAt: Date.now(), tick: null, limit: null, done: false }
  recording = state
  recorder.addEventListener('dataavailable', (event) => {
    if (event.data?.size) state.chunks.push(event.data)
  })
  recorder.addEventListener('stop', () => finish(state))
  recorder.addEventListener('error', (event) => {
    openremote.ui.toast(`The recording stopped: ${event.error?.message ?? 'the microphone failed'}`)
    stop()
  })
  // A microphone unplugged mid-sentence ends the recording with what it heard.
  for (const track of stream.getAudioTracks()) {
    track.addEventListener('ended', () => {
      if (recording === state) {
        openremote.ui.toast('The microphone went away; writing down what it heard.')
        stop()
      }
    })
  }
  recorder.start(1000)
  const listening = () =>
    show('active', `Listening ${clock(Date.now() - state.startedAt)}`, 'Listening - click to stop and write it in', composer)
  listening()
  state.tick = setInterval(listening, 1000)
  state.limit = setTimeout(() => {
    if (recording !== state) return
    openremote.ui.toast(`Stopped at ${minutes} min, the longest recording in Dictation's settings.`)
    stop()
  }, minutes * 60_000)
}

function stop() {
  const state = recording
  if (!state) return
  recording = null
  clearInterval(state.tick)
  clearTimeout(state.limit)
  // The recorder's own stop event writes it down, once the last piece of
  // audio is in.
  if (state.recorder.state !== 'inactive') state.recorder.stop()
  else finish(state)
}

async function finish(state) {
  if (state.done) return
  state.done = true
  if (recording === state) {
    recording = null
    clearInterval(state.tick)
    clearTimeout(state.limit)
  }
  // The microphone is let go at once: the system's indicator goes off
  // while the words are written down.
  release(state.stream)
  const type = (state.recorder.mimeType || 'audio/webm').split(';')[0]
  const audio = new Blob(state.chunks, { type })
  if (Date.now() - state.startedAt < SHORTEST_MS || audio.size < 1024) {
    show('idle', null, null, state.composer)
    openremote.ui.toast('That was too short to hear. Hold on a moment longer.')
    return
  }
  transcribing = true
  show('busy', 'Transcribing…', 'Writing down what you said', state.composer)
  try {
    const text = await transcribe(audio, state.settings)
    if (!text) {
      openremote.ui.toast('Heard nothing to write down.')
    } else {
      try {
        await openremote.composer.insert(text, { composer: state.composer })
      } catch {
        // The chat's composer is gone (the chat was deleted, say): the
        // words are not lost, they are shown instead.
        const shown = text.length > 200 ? `${text.slice(0, 200)}…` : text
        openremote.ui.toast(`The composer you spoke into has closed. You said: ${shown}`)
      }
    }
  } catch (err) {
    openremote.ui.toast(err.message)
    // A key or an address that is wrong is fixed in Settings: open it.
    if (err.inSettings) openremote.ui.openSettings().catch(() => {})
  } finally {
    transcribing = false
    show('idle', null, null, state.composer)
  }
}

const extension = (type) => (type.includes('ogg') ? 'ogg' : type.includes('mp4') ? 'mp4' : 'webm')

// The service's own words for a refusal, when it gives any.
async function reason(response) {
  try {
    const body = await response.text()
    try {
      const data = JSON.parse(body)
      const words = [data?.error?.message, data?.error, data?.message, data?.detail].find((w) => typeof w === 'string')
      return (words ?? '').slice(0, 200)
    } catch {
      return body.trim().slice(0, 200)
    }
  } catch {
    return ''
  }
}

// A failure the user fixes in Dictation's settings.
function settingsProblem(words) {
  const err = new Error(words)
  err.inSettings = true
  return err
}

async function transcribe(audio, { values, secrets }) {
  const base = String(values.base_url).trim().replace(/\/+$/, '')
  const form = new FormData()
  form.append('file', audio, `dictation.${extension(audio.type)}`)
  form.append('model', String(values.model ?? '').trim() || 'whisper-1')
  form.append('response_format', 'json')
  const language = String(values.language ?? '').trim()
  if (language) form.append('language', language)
  const prompt = String(values.prompt ?? '').trim()
  if (prompt) form.append('prompt', prompt)
  // OpenRemote writes the key in on the way out, toward the service
  // address only; with no key set, the header stays out.
  const headers = secrets.api_key === 'set' ? { Authorization: 'Bearer {{secret:api_key}}' } : {}
  let response
  try {
    response = await openremote.fetch(`${base}/audio/transcriptions`, {
      method: 'POST',
      headers,
      body: form,
      timeoutMs: 180_000,
    })
  } catch (err) {
    throw new Error(`Couldn't reach the transcription service: ${err.message}`)
  }
  if (response.status === 401 || response.status === 403) {
    throw settingsProblem(
      secrets.api_key === 'set'
        ? 'The transcription service turned the API key down. Check it in Settings.'
        : 'The transcription service needs an API key. Set it in Settings.',
    )
  }
  if (response.status === 404) {
    throw settingsProblem(`The transcription service has no ${base}/audio/transcriptions. Check its address in Settings: it usually ends in /v1.`)
  }
  if (response.status === 413) {
    throw new Error('The recording was too long for the transcription service. Try a shorter one.')
  }
  if (response.status === 429) {
    throw new Error('The transcription service is busy, or the account is out of credit. Try again in a moment.')
  }
  if (!response.ok) {
    const why = await reason(response)
    throw new Error(`The transcription service answered ${response.status}${why ? `: ${why}` : ''}.`)
  }
  const data = await response.json().catch(() => null)
  return typeof data?.text === 'string' ? data.text.trim() : ''
}
