# Composer Add and Tools menu

## Intent and agreed direction

The user wants the composer + button to offer the breadth and finish expected of a leading AI app, with missing actions implemented rather than a larger list of placeholders. They selected a unified Add and Tools menu over an attachment-only menu.

This is a composer feature spanning input, context collection and delivery. Preserve OpenRemote's existing dark appearance and its Apps vocabulary. The menu should make the aggregate's useful capabilities easy to reach from one place.

## Current implementation

- `useAttachInput` in `src/ComposerAttachments.jsx` opens an image-only file input. It renders no + control when the selected App cannot read images.
- New chat and existing chat both use that hook. Paste/drop, image conversion, limits and outgoing assembly live in `src/composer-attachments.js`.
- `src/attachments.js` stores pending items per composer. Drafts have a focused-composer ownership mechanism, and new-chat items move to the newly created chat.
- `src/useComposerMenus.jsx` already supplies / commands and @ mentions, including workspace files, chats and App-provided capabilities.
- `src/PluginComposerButtons.jsx` already dispatches real plugin actions with a grant belonging to the clicked composer and project.
- The built-in browser can share text, pick an element, capture the page and capture a region. Availability varies by platform. These actions currently live in its toolbar and menus, not the composer.
- The daemon attachment decoder currently accepts only images. A browser-selected document cannot be sent merely by changing the file input's accept attribute.

## Approaches

1. **Unified Add and Tools menu, selected by the user.** One shared entry point for content and available capabilities. Requires document ingestion and coordinated browser/plugin actions, but expresses the aggregation advantage directly.
2. **Attachment-only menu.** A smaller change containing pictures, documents and clipboard input, with commands and plugins left in separate controls. Less integration work, but makes capabilities harder to discover and does less to address the user's product direction.

## Goals

- Replace the direct file-picker click with an anchored, grouped menu in both composers.
- Make every offered action perform its named job, with clear loading, cancellation and recoverable error states.
- Add actual support for text documents, PDFs, Word documents and spreadsheets, with readable context delivered to every App that accepts prompts.
- Reuse working browser, mention, command and plugin paths rather than inventing parallel behavior.
- Preserve draft text, cursor location and attachment ownership across cancellation, async work, failed sends and App changes.
- Keep the experience equally understandable on desktop and phone.

## Menu content and behavior

The initial menu has two sections. Use plain sentence-case labels, small consistent icons, restrained separators and no promotional copy. Preserve the existing typography and palette; the menu inherits the current theme rather than introducing another visual system. The user's supplied ChatGPT and Claude phone screenshots are the visual references: a bottom sheet titled Add to chat, with Camera, Photos and Files as separate prominent actions, followed by quieter context and tool rows. Desktop uses a compact anchored menu with the same actions.

**Add**

- **Photos** and **Files:** separate pickers for images and supported documents. Images continue through the existing conversion path. Documents enter the new document ingestion path. Supported types are named in the picker/help text. Mixed drag-and-drop remains supported.
- **Paste from clipboard:** add clipboard images or a named text attachment when the platform exposes an explicit clipboard-read API. Permission denial explains that the user can paste directly into the composer; ordinary Ctrl/Cmd+V continues to work without a menu.
- **Take a photo:** on phones whose file input supports camera capture, open a separate image input requesting the rear camera. The operating system owns capture; cancellation leaves the draft alone.
- **Reference files or chats:** open the existing @ picker at the preserved cursor. This references files on the execution machine, whereas Files and photos imports selected bytes from the user's current device.
- **Browser context:** a submenu or second picker containing **Add page text**, **Screenshot page**, **Screenshot a region**, and **Pick an element** when the native browser provides them. If no page is open, show an explicit **Open browser** action first. Pick the page by title and origin if more than one page is eligible. A successful browser share keeps the existing browser-sharing behavior for the eventual chat.

**Tools**

- **Commands and skills:** open the existing / picker, using the selected App's own catalog, vocabulary and readiness states. Choosing an item prepares the draft exactly as the existing picker does; it does not send a message.
- Available plugin composer inputs, labeled with their real names and plugin identity. Selecting one invokes the existing click/grant path for this composer and project. Do not expose arbitrary agent MCP tools as directly executable UI commands.
- **Browse plugins:** navigate to the existing Plugins view when the user wants more capabilities. This action does not install anything.

Do not fabricate image generation, web search, research, voice or connectors as universal built-in modes. If an installed plugin contributes a corresponding composer action, it appears through the real plugin path. Existing visible plugin buttons may remain as quick controls, with shared state and dispatch rather than duplicated grants.

## Capability and lifecycle rules

- Keep + available for text-capable actions even when the selected App cannot accept images. Hide image-specific actions for those Apps, and clearly explain a mixed import's rejected image while retaining supported documents.
- Use the existing App capability inventory and native-browser feature inventory. Do not infer that every Tauri platform supports Windows browser actions.
- A stopped, unreachable or otherwise non-editable chat follows the existing composer rules; the menu cannot bypass them.
- Capture the initiating composer key for every async operation. Never deliver its result to whichever composer happens to gain focus later. Closing that composer cancels or discards unfinished work. Moving a new-chat draft into a chat must either finish imports first or transfer their ownership atomically.
- Async parsing gets a pending chip and cancellation/removal. Sending is blocked while selected inputs are still being prepared, with a visible reason. A failed item shows a removable error chip; the user removes or retries it before sending. Do not silently omit failed content.
- Two simultaneous imports share the same count and byte accounting. Removing an in-progress chip prevents a later completion from bringing it back.
- Menu opening and cancellation preserve the draft and selection. Choosing a text insertion restores focus and places the cursor correctly. A dismissed menu returns focus to +.

## Document ingestion and delivery

These are implementation proposals, not choices the user separately specified:

- Parse selected document bytes on the user's current device in the console, using maintained libraries loaded only when needed. The same path must work in desktop, phone and the web console, including when the App runs on another machine. Do not send a local path that the receiving machine cannot read.
- Support UTF-8 text, Markdown, CSV, TSV, JSON, logs and source files; text-based PDF; DOCX paragraphs and tables; and XLSX sheets converted to labeled rows. Explicitly reject legacy binary Office files and unsupported binaries. Preserve page numbers, document/table boundaries and spreadsheet sheet names where the formats provide them.
- Treat documents as a distinct pending attachment kind with filename, type, source byte size and extracted text. Chips show filenames and preparation status, offer remove/retry, and allow a plain-text preview of what will be sent.
- Assemble extracted content into clearly delimited, named document blocks after the user's typed text. Include document provenance and identify the blocks as reference content. Use the existing prompt delivery for send, queue and steer, so documents work across Apps without pretending the image-only daemon protocol supports generic files.
- Keep source images and browser images on the existing image attachment protocol. Do not change the meaning of the daemon's `honors.attachments` flag to imply document support.
- Document content appears in the stored user message as reference blocks, so reopening the chat does not require the original local file. The picker and preview must explain that document text is included with the message. This does not create a persistent file library.
- Initial limits: 10 pending attachments total; 10 MB source bytes per document; 200,000 extracted characters per document and 500,000 across documents in one draft. Reject content exceeding a limit with a concrete size/limit message; do not truncate without telling the user. Bound parsing/decompression as well as final output. Preserve existing per-image limits.
- Apply the message bound to the final assembled prompt, not only typed text. Validate the complete serialized outgoing request against the actual local or remote transport ceiling, including base64 images and envelope overhead. Existing image/remote limits may impose a lower practical size. Do not increase network body limits for this feature.
- Scanned/image-only PDFs explain that no readable text was found and suggest adding page images. Encrypted, corrupt or empty documents explain what happened and how to recover. DOCX images, PDF visual layout, spreadsheet formulas/styles/charts and macros are not represented as if extracted text preserved them. No OCR or external conversion service in this change.
- Parsing must not execute document content, load external resources or write document text into logs. Use public primary documentation when choosing parser APIs and versions. Confirm compatibility with the existing Vite/Tauri build and restrictive CSP.

## Visual and interaction contract

- Reuse `PickerMenu.jsx`, the project's dropdown engine, extending it only where necessary for action rows and section navigation. Use the existing modal family for document previews.
- Desktop: open above + when space allows, clamp to the available viewport and escape clipping via the existing portal behavior. Keep the menu compact, with a scrollable list for many plugin actions.
- Phone: use the existing mobile picker treatment, at least 44px action targets, and visual-viewport-aware positioning when the keyboard is open. The camera action is phone-specific.
- Keyboard: + has a descriptive accessible label and expanded state; arrows move through actions, Enter chooses, Escape dismisses the innermost surface, and focus returns predictably. Selecting a plugin action preserves its composer grant context.
- Respect both system and in-app reduced-motion settings. Async status and errors are announced without stealing focus.

## Scope boundaries

This change delivers the menu and the ingestion/context actions above. It does not introduce a new AI provider, connector authentication, a persistent asset library, operating-system desktop recording/screen capture, OCR, audio transcription or a release. Browser screenshots are explicitly labeled as browser/page capture. Preserve unrelated in-flight changes.

## Builder decisions

Choose parser packages, exact module boundaries and minor layout values during planning, after checking their primary documentation and bundle behavior. Prefer a shared action model with explicit availability and callbacks, a bounded document-ingestion module, and one lifecycle-aware attachment store. Settle ordinary implementation details without more product questions.

## Acceptance and verification

- + opens a real menu in new and existing chats. Selecting Files and photos opens the picker; opening or cancelling the menu does not.
- Each supported document type yields a named preview and the expected readable content in the actual outgoing payload. Local, remote, queue and steer delivery all use the same assembly rules.
- Mixed files, image-incompatible Apps, malformed/encrypted/scanned files, oversized input and pending/removal races behave as described without losing the typed draft.
- @ and / entries reach the existing pickers at the preserved selection. Plugin entries invoke the real scoped grant and show the same active/busy state as quick buttons.
- Browser actions attach the chosen page's result to the initiating composer, including a new chat, and disappear where unsupported. No remote chat receives an unusable local file path.
- Verify keyboard navigation, outside click, Escape, focus return, mobile keyboard height, long plugin names, reduced motion, and the actual rendered desktop and phone layouts.
- Run targeted behavior tests, the full console suite and both desktop/web frontend builds. Run relevant native/daemon checks only if those layers change. Record any native-camera or platform behavior that cannot be exercised on this machine.

## Research references

The comparison supports a familiar attachment entry point and discoverable connected capabilities; the scope and architecture above are OpenRemote-specific proposals.

- OpenAI, [Working with files in ChatGPT](https://openai.com/academy/working-with-files/): documents and images are available through the tools menu.
- Anthropic, [Upload files to Claude](https://support.claude.com/en/articles/8241126-upload-files-to-claude): document/image ingestion and text extraction have explicit format limits.
- Anthropic, [Use connectors to extend Claude's capabilities](https://support.claude.com/en/articles/11176164-use-connectors-to-extend-claude-s-capabilities): connected capabilities are discoverable from chat and retain their source permissions.

## Approval status

The user selected the unified-menu approach, then supplied ChatGPT and Claude phone references and instructed the agent to handle the work itself. This delegates remaining design and implementation decisions; no further approval gate is needed for this scope.
