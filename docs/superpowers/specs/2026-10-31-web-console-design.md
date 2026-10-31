# The web console

Status: design, being built. Owner: the console team.

## What is being asked for

The user signs in on the web and reaches the computers of their own account.
No cloud agents: nothing in moreweb's cloud runs a harness, and no cloud
process ever holds the user's keys.

The backend is the moreweb gateway at `api.moreweb.space`, which already
serves OpenRemote Cloud: the OAuth client, the device registry, the
enrolment codes and the relay. The console at
`openremote.moreweb.space` is the front end, and it is a static build with
no server of its own.

## The rule this must not break

The relay's contract, in `Projects/daemon/src/services/openremote/relay.rs`,
is that it forwards frames between the devices of one account and never
reads, stores or logs a payload. Devices encrypt end to end; the relay sees
noise.

A web console can break that in one obvious way: have the gateway hold a
key pair per signed-in browser and decrypt on its behalf. The gateway would
then read every message, and the product's promise would become false for
everyone using the web version. This design does not do that.

## The browser is one of the user's devices

The Android app already proves the shape: an OpenRemote daemon with no
harnesses whose only job is to drive the account's machines. A browser can
take that same seat, because every part it needs already exists:

- `openremote-desktop` is a public OAuth client with PKCE, so a browser can
  sign in and hold the account's access token.
- `POST /v1/openremote/devices` registers a device from an account token and
  returns a device credential (`mwd_...`) once. `valid_platform` already
  accepts `web`, and a device that is not a machine is exactly what a
  browser is.
- `GET /v1/openremote/devices` returns each device's Noise public key, which
  is what a Noise KK handshake needs for its peer.
- `GET /v1/openremote/relay` is a WebSocket of `[16-byte destination][payload]`
  frames, and `openremote_cloud::link` is the Noise KK link over those
  frames.
- The peer allowlist in the daemon (`PEER_ROUTES`, three tiers) already
  decides what a non-machine device may ask of a machine.

So the browser keeps its own X25519 key pair in the browser, registers as a
device, opens the relay with its device credential, and speaks the same link
protocol the desktop speaks. The gateway keeps relaying bytes it cannot
read.

## What the browser needs that a daemon gets for free

Two things:

1. **The link layer.** `snow` and the framing in `openremote_cloud::link`
   are Rust. Reimplementing Noise KK, ChaChaPoly and BLAKE2s in JavaScript
   to match byte for byte is the one part of this that must not be
   hand-written: a single mismatch is an intermittent handshake failure
   that looks like a flaky network.

   The same protocol code is therefore compiled to WebAssembly:
   `daemon/crates/openremote-webpeer` is a `cdylib` for
   `wasm32-unknown-unknown` that reuses `openremote_cloud::{identity, link}`
   and `openremote_core::DeviceId`. No `wasm-bindgen`: the crate exports a
   small C ABI and the JavaScript glue handles its own memory. That keeps
   the build to `cargo build --target wasm32-unknown-unknown` with no extra
   toolchain, and keeps the protocol in one place.

2. **A relay socket a browser may open.** A browser's `WebSocket` cannot set
   an `Authorization` header, and the relay only accepts a device credential
   in one. The credential therefore travels in the WebSocket subprotocol
   (`Sec-WebSocket-Protocol: cred.<token>`), which browsers do allow, and the
   gateway accepts it there as well as in the header (the header stays for
   the daemons, and stays checked first). The token never lands in a URL,
   where a proxy would log it.

## The JavaScript and wasm boundary

The module owns crypto and nothing else. `src/peer/webpeer.js` owns the
WebSocket, the request stream ids and the storage.

```
wp_alloc(len: u32) -> u32             a buffer in linear memory
wp_free(ptr: u32, len: u32)
wp_new(priv_ptr: u32) -> u32          handle, 0 when those 32 bytes are no key
wp_pubkey(h: u32) -> u32              pointer to 32 bytes
wp_initiate(h, peer_pub, account, account_len, self_id, peer_id) -> u32
wp_respond(h, peer_pub, account, account_len, self_id, peer_id, hs1, hs1_len) -> u32
wp_complete(h, hs2, hs2_len) -> i32   0 up, 1 refused
wp_seal(h, ptr, len) -> u32
wp_open(h, ptr, len) -> u32
wp_len() -> u32                       length of the frame the last call returned
wp_error() -> i32                     0 none, otherwise why
```

A returned pointer with `wp_len()` is one relay payload, type byte
included, ready to send. `wp_open` returns the plaintext, and reports a
failure through `wp_error` rather than by returning nothing, so a dropped
frame is never mistaken for an empty message. Buffers a call returns stay
valid until the next call. No panic crosses the ABI: a failure sets the
error and returns 0.

`account`, `self_id` and `peer_id` are the Noise prologue of `link.rs`; the
JavaScript side passes the account id string and both ids as 16 raw bytes.

Request framing stays in JavaScript (`rpc.rs` is five bytes of header and a
chunk size), because it is a data format with no crypto in it.

## What changes where

`Projects/daemon` (the gateway, deployed at api.moreweb.space):

- the relay accepts a device credential from the subprotocol;
- the `openremote-desktop` client's registered redirects gain the console's
  origin, so a browser can complete the same PKCE sign-in;
- a device may register as `kind: "web"`, so the device list can say what it
  is and the "is this a machine" question stays honest;
- tests for all three.

`Projects/OpenRemote` (the console):

- `daemon/crates/openremote-webpeer`: the crate above, with its own tests
  (two modules link to each other in a unit test, the way `link.rs` does);
- `src/peer/`: the glue, the device identity in IndexedDB, registration,
  the relay socket, request streams, and a node test that drives the wasm
  module;
- `src/api/daemon.js`: the one seam. `DaemonApi.call` is the only `fetch` in
  the console, so the web transport subclasses it and answers the same
  shapes. Nothing else in the console learns that it is in a browser;
- a second Vite entry, `web.html`, which mounts the console with the web
  transport, signs in through moreweb, and offers the account's machines.

`openremote.moreweb.space` (the static front end): the built console, put
where the site repo's router already serves the product pages.

## Out of scope

- Cloud agents, or hosting harnesses anywhere but the user's own machines.
- Any server-side key, session decryption or message logging.
- A hosted relay for people who are not signed in to moreweb.
- Live streaming in the first cut: the peer allowlist already carries
  `GET /sync/sessions/{id}/events` for polling a range, and the console's
  SSE reader keeps working where it is (local mode) until the stream is
  carried over a link.

## How it is verified

- `openremote-webpeer`: a unit test links two instances in Rust, and a node
  test loads the built `.wasm`, links it to a Rust peer, and exchanges a
  sealed request. This is the byte-for-byte guarantee.
- The gateway: an integration test that opens the relay with the credential
  in the subprotocol, and one that registers a `web` device and reads the
  device list back.
- End to end, on this machine: sign in at the console origin, see the
  machine, and read its chat list through the link. Then the console gates
  and the daemon gates stay green, which is the condition for keeping any of
  it.
