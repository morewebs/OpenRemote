# What another device of your account can do to this computer

Cloud mode joins the user's own computers through moreweb's relay, end to end encrypted. Every device
of that account can reach every other device's daemon API. This page lists, route by route, what that
means in practice: what a peer can make this computer do, and what it cannot.

The gate is code, not policy: `peer_gate` in `daemon/crates/openremote-daemon/src/http.rs` serves the
same handlers the console uses, but only for the routes in `PEER_ROUTES`. If you change that list,
change this page in the same commit. A route that forwards a body the peer chose is a row, because
the peer picked every argument in it.

## How a peer's request arrives

1. The peer's daemon opens a sealed link to this one over the relay (`openremote-cloud/src/mesh.rs`).
   The link's Noise handshake, against a key pinned from the registry, is what proves the caller is a
   device of the account (`mesh.rs`, `update_devices`).
2. The request is handed to `peer_router` (`host.rs`), which runs it through `peer_gate` and then
   through the normal handler.
3. The identity the handler sees is the link's device id, plus the name and kind the registry holds
   (`PeerContext`, `mesh.rs:26-32`). The daemon token plays no part on this path.

There is no per-request human confirmation and no flag saying "a person clicked this". The console's
own "start this chat on my machine" flow sends exactly the same body over exactly the same channel
(`cloud_http.rs`, `create_remote`), so the receiving daemon genuinely cannot tell the user's click
from a peer's client. That is a property of the design, not an oversight to patch quietly.

## The three tiers

| tier | what the gate checks on top of the pinned link |
|---|---|
| Sync | nothing. Any device of the account may copy synced chat logs, and the account's Global plugin records and copies. |
| Machine | this computer must be a machine. The check is on the receiver (`mesh().kind() == "machine"`), not on the caller: a phone may drive a machine. |
| Chat | this computer must be a machine, and the chat must be synced and executing here. A private chat answers 404, the same answer as a chat that does not exist. |

## The rows

34 rows: 4 Sync, 17 Machine, 13 Chat. "Sessions" and "chats" are the same thing; the API calls them
sessions.

### Sync tier (4)

| route | tier | what a peer can do | what it cannot |
|---|---|---|---|
| `GET /sync/summary` | Sync | list every synced chat on this computer: id, executor device, head, updated_at, plus deletion tombstones | see a private chat at all: the store filters to chats with an executor (`store.rs`, `summaries`) |
| `GET /sync/sessions/{id}/events` | Sync | read a synced chat's event log verbatim, from an event number (`seq >= from`, `store.rs`): the whole transcript, tool inputs and outputs included | read a private chat (404, `sync.rs`, `range`), read a tombstoned chat (410), or write anything into the log through this route |
| `GET /sync/plugins` | Sync | list the account's Global plugin records this computer holds: name, marketplace, version, runtime, the powers the installing device approved, the content hash, removed or not (`plugin_runtime/global.rs`) | install anything, here or on the caller: a record is shown to the user on the caller for review |
| `GET /sync/plugins/{id}/archive` | Sync | copy the files of a plugin installed here for every device, in slices, only while they are exactly the bytes the caller names | copy a plugin installed for this machine or a project (404), another version (409), its settings or secrets (kept outside the plugin's folder) |

### Machine tier (17)

| route | tier | what a peer can do | what it cannot |
|---|---|---|---|
| `GET /capabilities` | Machine | learn this machine's daemon version, harnesses, platforms, and what the daemon can do | change anything |
| `GET /machines` | Machine | see the machines this daemon knows, with their synced chats | see a private chat: `for_peer` filters each machine's session list to chats that run here (`http.rs`) |
| `GET /harnesses/{id}/models` | Machine | make this machine probe a harness for its model list (the probe starts the harness CLI briefly) | choose a model, or talk to a harness that is not installed |
| `GET /fs/dirs` | Machine | list directories anywhere the daemon's user can read, for the folder picker | read a file's contents |
| `GET /commands` | Machine | ask a harness for its slash commands and agents, read through the harness's own backend here (the same list the console's `/` menu shows) | run one |
| `GET /files` | Machine | enumerate file and folder names under a folder it names, or under this computer's home folder by default: paths only, ranked for a query, truncated | read file contents; the walk lists names |
| `POST /harnesses/{id}/signin` | Machine | start a harness's own interactive login on this machine | complete it alone (see the next row), or read the credential afterwards |
| `GET /harnesses/{id}/signin` | Machine | poll that login's view, including the vendor's words and any URL or code it shows | - |
| `POST /harnesses/{id}/signin/input` | Machine | type answers into that login prompt, so the harness on this computer ends up authenticated as whatever the peer chose | get at an existing credential: that is a fresh login |
| `POST /harnesses/{id}/signin/stop` | Machine | abandon that login | - |
| `POST /machines/{id}/harnesses` | Machine | install a harness CLI on this computer, running its owner's installer (and, with `with_runtime`, a Node runtime first) | install something that is not a known harness, or install one already present (409) |
| `POST /sessions` | Machine | create a chat here: harness, folder, model, effort, model endpoint, and the harness's permission word. A chat a peer creates is synced | name another device (400, a chat runs where it is created), create on a phone (409), use a folder that is not an existing absolute path (422), or create a private chat |
| `GET /projects` | Machine | list this machine's projects | - |
| `POST /projects` | Machine | add or update a project entry (store metadata only) | point a project at a folder that is not an absolute existing directory |
| `DELETE /projects/{id}` | Machine | delete a project entry | delete the folder on disk |
| `GET /model-endpoints` | Machine | list custom model endpoints: names, harnesses, base URLs, model ids | see their API keys: the store hands metadata only (`http.rs`, `list_model_endpoints`) |
| `GET /model-endpoints/{id}/models` | Machine | make this machine fetch the endpoint's live model list (a network call with the stored key, the result and the key are not returned) | read the key, or use it against another address |

A harness a plugin provides is a machine harness like any other: once the
plugin is installed here, `GET /capabilities` lists it and a peer may start a
chat on it through `POST /sessions`. Installing a harness plugin is therefore
also a choice about what this computer will run for every device of the
account.

### Chat tier (13)

All 13 require a machine, and a chat that is synced and executing here. What they share: none of them
can touch a private chat, create or delete a chat, change its folder, or read a log through these
paths (the Sync tier reads logs, the note channel appends them; see One-way notes).

| route | tier | what a peer can do | what it cannot |
|---|---|---|---|
| `POST /sessions/{id}/prompts` | Chat | send a message into the chat, which starts a turn on this machine | pick the model or the permission word for that turn; those are the chat's own settings |
| `POST /sessions/{id}/queue` | Chat | queue a message behind the running turn | bypass the queue |
| `POST /sessions/{id}/queue/resume` | Chat | release a queue the user held | - |
| `POST /sessions/{id}/queue/{item}/remove` | Chat | drop one queued message | - |
| `POST /sessions/{id}/queue/{item}/steer` | Chat | move a queued message into the running turn | - |
| `POST /sessions/{id}/steer` | Chat | inject a message straight into the running turn | - |
| `POST /sessions/{id}/settings` | Chat | change model, effort, fast, and the harness's permission word. Permission is start-time only: on a live chat it is refused (409), on a stopped chat it is stored and the next resume starts the harness with it | apply a permission word to a running harness; that is the point of the 409, and it is not a protection against a peer who is willing to stop the chat first |
| `POST /sessions/{id}/endpoint` | Chat | point a stopped chat at one of the custom endpoints already stored on this machine, or back at the harness's own provider | invent an endpoint: `POST /model-endpoints` is not a peer route |
| `POST /sessions/{id}/interrupt` | Chat | interrupt the running turn, keeping the chat alive | - |
| `POST /sessions/{id}/stop` | Chat | stop the chat's process | - |
| `POST /sessions/{id}/resume` | Chat | start the chat again, which re-passes its stored model and permission word | choose new settings as part of the resume |
| `POST /sessions/{id}/title` | Chat | rename the chat, which every device shows | - |
| `POST /decisions/{id}/answer` | Chat | answer any pending approval or question in the chat: allow, deny, or an "always allow" that stores a tool grant for the chat | answer a decision belonging to a private chat (404), or forge a decision id that is not pending here |

## One-way notes, which are not rows

Some of what a peer can do is not a route. The mesh also carries notes, handled by `on_note` in
`daemon/crates/openremote-daemon/src/sync.rs`, so `PEER_ROUTES` does not gate them and no tier
applies. Five types reach it. Four of them are about chats, and none of the four can touch a private
chat; the fifth carries no chat at all and is the one described after the table. A sixth,
`plugins.global`, is handled beside them by `plugin_runtime/global.rs` and is described last.

| note | what it does on this computer | a private chat |
|---|---|---|
| `sync.watch` | subscribes the sender to a chat's live events, so its turns stream to that device. Accepted only when this computer runs the chat (`executes_here`); the same note with `on: false` unsubscribes | ignored: a private chat has no executor, so the note fails the `runs_here` check |
| `sync.events` | appends events to a chat's log, attributed to the sender. The store takes them only from the chat's executor and only in sequence, and a gap makes this computer pull the missing range (`store.rs`, `append_replicated`) | refused: a private chat names no executor, so the attribution cannot match |
| `sync.head` | makes this computer pull from the sender when the sender's head is ahead of its own (`pull`) | nothing happens: it writes nothing itself, and what comes back is checked the same way as an append |
| `sync.tombstone` | deletes a synced chat: stops it if it is running here, records the tombstone, and stops syncing it (`apply_tombstone`) | returns before it stops, tombstones or forgets anything |
| `sync.browser` | rewrites this computer's browser settings and bookmark tree: every row newer than the local one is taken, key by key, deletions (a bookmark removed) included. Any device of the account may send it (`browser/sync.rs`, `on_note`) | nothing at all: the note names no session, so it never reaches chat data, and history, cookies, site storage and the tabs to restore are not in it |

| `plugins.global` | keeps the account's newer record of a Global plugin. A live record installs, updates and grants nothing: the user is shown it under Plugins, and installing it means fetching the copy, checking it is exactly the recorded bytes, reviewing it and approving it here, where its powers are read from the copy. A removed record takes this computer's Global copy of that plugin off, with its settings, unless a chat runs on its App; a copy installed here for this machine or a project is never touched | nothing at all: the note names no session |

The tombstone is a deletion, and it is the one that matters. It is also how a deletion crosses
devices: the computer where a synced chat was deleted sends one, the copies apply it, and that is why
the chat disappears on the other device. A peer that sends one deletes any synced chat id here,
running or stopped, and the code does not check that the sender is that chat's executor. It also
lands on an id this computer never held: the tombstone is recorded anyway, and a chat with that id
that syncs here later is refused from then on. Nothing in the route table above does that, and no
route undoes it: after a tombstone, `/sync/sessions/{id}/events` answers 410 and every summary skips
the chat, so deletion here is a one-way door.

A tombstone also carries the time it was made, and nothing compares that time with the row it lands
on: a deletion kept here because the chat would not stop, or simply offered again on the retry
beat, removes a chat that has been worked in since. A delete only wins if it is newer than the work
it destroys, and today the code does not ask. The comparison belongs in `Store::tombstone` so every
writer path gets it, with a local delete always stamping the present.

One thing follows that a reader should not have to discover: `sync.events` naming an id this
computer does not hold is not only an append. The store accepts a copy that opens with a
`session.created` and names the sender as its runner, so a peer can make a synced chat appear in
this computer's Cloud list, the same shape the Sync tier's pull already allows.

`sync.browser` is the same kind of door as the tombstone, one collection further out. Any device of
the account may send it, and it writes this computer's browser settings and bookmark tree without
asking: a peer can set a value, change one the user wrote, add a bookmark or delete one, and the
result is what the next person at this computer sees. Nothing here records which device changed what,
and no route undoes it. What it cannot reach is on the other side of the same table: the note names
no session, so no chat's data takes part, and the four collections that stay device-local (history,
cookies, site storage, the tabs to restore) are not in the note at all. The merge is per key by
stamp, so every device converges on the newest write rather than the last one to arrive, and a note
a version behind cannot be mistaken for a chat one; none of that is a protection against a peer who
simply means it. Removal is: a device taken off the account stops being a device of the account, and
its notes stop arriving with its key.

The note is bounded, which is what stops a broken or hostile peer from growing this computer without
limit. One note may carry at most 5,200 rows (the two collections' ceilings together) and no row's
value may exceed 4 KiB; a note over either ceiling, or one introducing more keys than a collection
can hold, is ignored whole with a named reason (`Refused` in `browser/sync.rs`) rather than half
applied. The store itself holds at most 200 settings and 5,000 bookmarks: at a ceiling a new key is
refused and the rows already held stay. Dropping the oldest by stamp was the alternative and it is
the wrong door, because it would let a peer evict a bookmark the user made by inventing a newer one.
A local row above the value ceiling is left out of this device's own snapshot for the same reason: a
peer refuses the whole note for one, and sending it would stop this device's sync to say nothing. The
numbers sit far above a real profile (tens of settings, and bookmarks in the hundreds to low
thousands) and are pinned by a test, so raising one is a decision rather than a tweak.

The private-chat refusal is by construction in each of the four chat paths, not one shared check.
Only the tombstone path has a test (`sync.rs`,
`a_tombstone_naming_a_private_chat_is_refused_and_its_data_survives`); the other three are read
from `on_note` and `append_replicated`. The browser note goes a different way: it names no session
to refuse, its merge rules are unit tested (`browser/sync.rs`), and two daemons sharing a setting
and a bookmark deletion are driven end to end in `tests/e2e_browser_sync.rs`.

## The uncomfortable part

The approval gate guards against a harness, not against a device.

Proven end to end against this code: a machine of the same account can

- start a chat here with an approval-bypassing permission word (`POST /sessions` with
  `permission_mode: "bypassPermissions"`, which the harness's own `--permission-mode` receives; the
  word is the harness's, and the console offers exactly these words in its picker),
- convert a stopped chat to that word and let the next resume use it (`POST /sessions/{id}/settings`,
  which stores it because permission is start-time on every harness; `supervisor.rs`,
  `update_settings`; the console path is pinned by `tests/e2e.rs`,
  `a_picked_permission_word_survives_the_init_echo_and_resume`),
- and answer the prompts the harness asks, including "always allow", through
  `POST /decisions/{id}/answer`.

So a prompt that appears to be the user's approval may be an approval by any device of the account,
and a chat may run with approvals off before the user ever sees it. A future contributor deciding
what is safe to forward should assume a peer can do all of the above; the allowlist keeps them from
the rest of the daemon's API (59 route registrations in `api_routes`, of which these 32 pairs are
peer rows), not from driving a chat.

## What actually protects the user in Cloud mode

- They chose the devices. Joining the account takes a sign-in in that device's system browser, or an
  enrollment code minted by a device already on it. Nothing joins silently.
- Removal is real. A revoked device's synced chats are purged here (`host.rs`, `revoked`), and its
  key stops matching the pin.
- Private chats stay private. They never leave the computer, are invisible to peers on every route
  above, and the answer is 404 rather than 403 so a peer cannot even learn one exists. The note
  channel refuses them too (see One-way notes).
- A chat runs where it was created, and only on machines. A desktop that is not a machine refuses to
  be driven at all (409 `not_a_machine`). A phone cannot run chats.
- Every action lands in the chat's own event stream: prompts, queueing, interrupts, settings
  changes, decision requests and answers are events the user's console renders.
- The console talks only to its own daemon on 127.0.0.1, with an owner-only token. A peer is never
  the console.

## What does not protect the user

- The approval gate, as above: it is a gate against a harness that asks, not against a device that
  answers.
- Attribution. Events record what happened, never who did it: no event carries a device id
  (`openremote-core/src/events.rs`, `EventPayload`), so "a prompt arrived" cannot be traced to the
  device that sent it. The event stream is an audit trail of actions, not of actors. Adding peers to
  it would be a design change.
- The permission word as a safety control. It is the harness's vocabulary, chosen by whoever creates
  or stops the chat, and no layer checks that the word is a cautious one.
- Anything on this page being "not available to your other devices". The gate is a list; a route
  added to `PEER_ROUTES` is open to every device of the account from that commit onwards, with no
  second check. The note channel is not in that list at all, so no "cannot" column above describes
  what a note can do (see One-way notes).
- The pins file as a last line of defence: `pins()` treats an unreadable `peers.json` as no pins at
  all, so a corrupted file re-pins whatever the registry serves (`mesh.rs:335-340`). It is a detection
  mechanism, not an authority.

## Rows that look dangerous and are not reachable by a peer

Kept here so the next reader does not have to re-derive it. These are console routes: a peer asking
gets 403 (`not available to your other devices`), and Cloud's own routes are not routed for a peer at
all (404, pinned by `tests/e2e_cloud_mesh.rs`). This section is about routes. Deleting a chat is not
refused outright: the route below is, and a peer can still delete a synced chat here through the
`sync.tombstone` note in the section above, which is also a deletion and also one-way. What is
refused on every path, route and note alike, is touching a private chat.

- Deleting a chat through its route (`DELETE /sessions/{id}`), reading a chat directly (`GET /sessions/{id}`,
  `GET /sessions/{id}/events`), listing chats (`GET /sessions`), archiving, and `POST /sessions/{id}/sync`.
- Plugins, marketplaces, MCP servers, automations, receipts, and browser routes.
- Writing model endpoints (`POST /model-endpoints` and its siblings), deleting a machine
  (`DELETE /machines/{id}`), and every Cloud route (`/cloud/...`).

## Unverified on this page

- The sign-in relay rows (`POST /harnesses/{id}/signin` and its input) are read from the code, not
  exercised end to end: no test drives a peer through a vendor login. The capability follows from the
  handler and the tier, but the exact vendor behaviour behind it was not reproduced.
- The `with_runtime` half of harness install (pi's Node setup) is read from `install.rs` and
  `supervisor.rs`, not run.
- `GET /model-endpoints/{id}/models` making a network call with the stored key is read from the
  handler and the endpoint store, not observed against a real gateway.

## If you are changing this

- Adding a route to `PEER_ROUTES` gives every device of the account that capability. Add the row here
  in the same commit, name what it cannot do, and ask whether a phone should have it.
- A route that forwards a body the console composed is still a peer row: the peer composes the body.
- If you add an actor to the event stream, this page's "what does not protect the user" section
  changes with it.
