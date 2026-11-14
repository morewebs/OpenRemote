# Add to chat

The + control opens one menu in New chat and existing chats. On phones it is a bottom sheet with separate Camera, Photos and Files actions. Desktop uses an anchored menu.

Files accepts UTF-8 text and source files, Markdown, CSV/TSV, JSON, text PDFs, DOCX and XLSX. Parsing happens on the selecting device. Document chips show preparation, preview, retry and removal; the extracted text is included as named reference content in the message. Images keep the existing image protocol. Apps without image support can still receive documents.

Documents are limited to 10 MB each, 200,000 extracted characters each and 500,000 characters together. A draft can contain 10 attachments. The assembled message must also fit the existing local or peer request limit. Scanned PDFs, encrypted files and unsupported binaries explain what could not be read; there is no OCR or external conversion. Text extraction does not retain charts, document images or spreadsheet formatting.

The same menu reaches the existing @ references, / commands and skills, available effort control and scoped plugin actions. The native browser contributes page links, page text, screenshots, region capture and element picking only where its reported capabilities support them. Opening the menu does not grant plugin access or send a message.

Split panes have independent composer identities. Changing projects or machines ends old plugin grants. A first prompt that fails after opening its chat restores the typed request and prepared attachments there. Queued reference content stays inert to OpenRemote's commands and chat-reference grants when edited and sent again.

## Verification

Run the ordinary checks from the repository root:

```sh
npm test
npm run build
npm run build:web
```

The browser smoke check uses the real React views, document workers, sandboxed plugin bridge, and WebConsoleProvider with sample App/account/peer responses. With `npm run dev` running and an isolated Chromium DevTools endpoint available:

```sh
node scripts/composer-smoke.mjs
```

`OPENREMOTE_CDP_URL` defaults to `http://127.0.0.1:9444`; `OPENREMOTE_UI_URL` defaults to `http://127.0.0.1:5175`. Screenshots and temporary sample documents go under `.scratch/composer-qa`. The check closes only the browser tab it creates. It sends no messages to an actual App or account.

The check covers narrow/typical phone and desktop menus, reduced viewport height, keyboard dismissal/focus, separate file chooser paths, document imports and preview, local send/queue, first-send recovery, split cancellation, late clipboard results, a real plugin frame and project-grant revocation, and photo-only/mixed delivery through the web provider's peer transport. The production document workers were also exercised from both desktop and `/app/` web bundles.

Physical camera capture, OS clipboard permissions and native browser webviews still need device testing. The smoke check validates the camera picker request and browser-independent flows, not the hardware result.
