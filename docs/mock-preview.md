# Live UI with sample data

Run `npm run dev:mock` and open <http://127.0.0.1:5176/#new>.

This runs the actual app entry point, React components, CSS, routing and Vite
hot reload. Save a UI change and it appears immediately. There are no copied
pages, alternate components or builds to keep in sync. Only the HTTP API is
mocked by `scripts/mock-preview/backend.mjs`, through a development-only Vite
middleware in `scripts/mock-preview.mjs`.

The preview starts with sample projects, chats, transcripts, installed Apps and
model choices. Creating chats, sending messages, renaming/deleting chats,
changing chat settings, registering projects and editing agent settings use
in-memory data. Chat replies arrive through the real client's SSE connection.
File attachments still use the app's real local document-ingestion code.

Port 5176 has separate browser storage from the real dev server on 5175.
The preview supplies a dummy connection and skips onboarding automatically.
It neither reads real credentials nor starts an agent or daemon. State resets
when the mock process restarts. UI preferences remain in the preview origin's
browser storage.

For empty, error or Cloud states, reset the sample backend from the browser's
developer console, then reload:

```js
await fetch('/__mock/__preview/reset', {
  method: 'POST',
  body: JSON.stringify({ scenario: 'empty' }),
})
location.reload()
```

Scenarios: `default`, `empty`, `error`, `cloud`. They affect sample API data,
not the real app's behavior. Unimplemented backend actions return a readable
501 error rather than claiming success. Add API fixtures as new backend-driven
features need them; frontend-only changes need no fixture updates.

This is for inspecting and iterating on UI. Continue using `npm run dev` with
a real backend or `npm run tauri:dev` to validate actual app integrations,
agent behavior, native dialogs, browser webviews and phone pairing. Normal dev
and production do not load the mock middleware or sample data.

Verify the mock API contract with `node --test scripts/mock-preview.test.mjs`.
