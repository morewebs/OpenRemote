// A phone's empty home must distinguish an unavailable computer from one
// that answered with no Apps. An unanswered list is not an installed-App count.
import { pickable } from './harness-manager.js'

export function mobileHomeState({ machine, apps, devicesAnswered = true }) {
  if (!machine && !devicesAnswered) return {
    kind: 'loading-machines',
    title: 'Finding your computers',
    detail: 'Waiting for your computer list. You can write your task while you wait.',
    action: null,
  }
  if (!machine) return {
    kind: 'no-machine',
    title: 'Connect a computer',
    detail: 'Your chats run on your computer. Connect one to start working from your phone.',
    action: 'machines',
  }
  if (!machine.online) return {
    kind: 'offline',
    title: 'Your computer is offline',
    detail: 'Open OpenRemote on your computer, or choose another one. Your draft will stay here.',
    action: 'machine',
  }
  if (apps == null) return {
    kind: 'loading',
    title: 'Connecting to your computer',
    detail: 'Getting its Apps ready. You can write your task while you wait.',
    action: null,
  }
  if (apps.some(pickable)) return null
  if (apps.some((app) => app.available)) return {
    kind: 'disabled-apps',
    title: 'Turn on an App',
    detail: 'Your Apps are turned off. Enable one in Plugins on your computer to start a chat.',
    action: 'machines',
  }
  return {
    kind: 'no-apps',
    title: 'Set up your first App',
    detail: 'Install an App such as Claude Code or Codex on your computer to start a chat.',
    action: 'machines',
  }
}
