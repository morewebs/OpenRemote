// The plain half of chat-images.jsx: how a fetched picture is keyed and what
// a picture that could not be fetched says. No DOM, so it is tested.

import { PICTURE_TOO_LARGE_TO_FETCH } from './api/daemon.js'

/** One picture's cache key: the device the client aims at, the chat route
 *  it asks through (another device's), and the picture's hash. */
export function pictureKey(prefix, session, blob) {
  return `${prefix ?? ''}|${session ?? ''}|${blob}`
}

/** What a picture that could not be fetched says, on hover. */
export function pictureMissing(err, elsewhere) {
  if (err?.status === 413) return `${PICTURE_TOO_LARGE_TO_FETCH}.`
  if (err?.status === 404) return 'This picture is no longer kept.'
  if (elsewhere) return "The device that keeps this picture can't be reached."
  return 'The console could not load this picture.'
}
