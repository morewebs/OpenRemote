# Mobile home

The user asked for a mobile homescreen redesign using the attached Claude and ChatGPT screenshots as references, then explicitly chose "Just go ahead with your recommendation."

Use a chat-first screen in the existing React console. Keep the dark OpenRemote palette and Inter typography. The welcome occupies the open space; the composer stays at the bottom of the available viewport, including when Android reserves space for the keyboard. Use 16px side margins and touch targets of at least 44px. Avoid the desktop glow and floating shortcut bar on this screen.

Keep App selection visible inside the composer. Put model, effort, permissions and saved setups behind a clearly named Chat options control. Keep the selected machine and project directly above the composer. Move Browser to the mobile header beside the menu and a quieter OpenRemote title; keep About accessible. Do not remove desktop features or change desktop layout.

Distinguish an unanswered machine inventory, a missing machine, an offline machine, an unanswered App list, disabled installed Apps and a genuinely empty App list. Each unavailable state gets one concise explanation and a relevant action when one can help. Never tell a phone user to install an App on their phone or imply an offline machine has no Apps. Keep portalled controls such as Effort inside the available viewport.

Palette: background #121212, composer #232323, primary text #e8e8e8, secondary text #a3a3a3, accent #a39bb8, online #4cb487. Use a 24px medium welcome, 16px input and 13px selectors. Keep the input compact for an empty draft, growing for multiple lines. A filled send control is the strongest action.

Alternatives: a recent-chats-first screen would favor resuming work, but the supplied references and the user's direction favor a quiet new-chat screen. Existing chats remain in the drawer. Reusing the centered desktop composer would retain the problem shown in the screenshots.

Verify the actual rendered page at narrow and typical phone widths, reduced keyboard height, long machine/model names, offline/no-machine/loading/empty-App states and the desktop layout. Do not publish a new release as part of this change.
