//! The browser half of OpenRemote's end-to-end link, as a plain C ABI.
//!
//! Every byte of protocol logic lives in Rust: this crate wraps
//! [`openremote_noise`] (the same Noise KK the desktop daemons speak) in small
//! entry points a browser can call on WebAssembly linear memory. The JavaScript
//! side owns everything that is not crypto: the device identity, the relay
//! socket, the request framing, and the storage.
//!
//! There is no `wasm-bindgen` on purpose. The build is
//! `cargo build --target wasm32-unknown-unknown`, with no extra toolchain, and
//! the ABI is small enough to bind by hand; a bindgen layer would add a build
//! tool for eleven functions.
//!
//! ## Calling conventions
//!
//! - Pointers are byte offsets into linear memory. `wp_alloc` makes a buffer the
//!   caller writes into and `wp_free` gives it back.
//! - A call that answers with bytes returns a pointer, and `wp_len` is the
//!   length of what it returned. That buffer is owned by this module and stays
//!   valid until the next call, so the caller must copy it before calling again.
//! - Frames come and go with their relay type byte (1 handshake, 2 response,
//!   3 message), exactly as they travel: the caller seals and passes what it
//!   received without knowing the protocol.
//! - A failure sets `wp_error` and returns 0. `wp_open` in particular reports
//!   failure only through `wp_error`, so an empty plaintext and a dropped frame
//!   can never be confused.
//! - No panic crosses the boundary: every entry point is guarded, and the code
//!   deliberately avoids the usual triggers (no unwrap on caller data, no
//!   indexing by an unvalidated offset). A WebAssembly build without unwinding
//!   turns a panic into a trap rather than a catchable payload, so the guard is
//!   a backstop on the host and the care below is what holds in the browser.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use openremote_core::DeviceId;
use openremote_noise as noise;

/// No error.
pub const ERR_NONE: i32 = 0;
/// The bytes are not a usable private or public X25519 key, or a device id.
pub const ERR_KEY: i32 = 1;
/// The handle is unknown, or a frame arrived at a link that is not up.
pub const ERR_STATE: i32 = 2;
/// The handshake refused the peer: wrong key, or another account's prologue.
pub const ERR_HANDSHAKE: i32 = 3;
/// The frame was too short or was not of the type its step expects.
pub const ERR_FRAME: i32 = 4;
/// A panic was caught at the boundary.
pub const ERR_PANIC: i32 = 5;

/// A pointer into linear memory. WebAssembly addresses are 32 bits; the host
/// tests link this crate as a library on a 64-bit machine, so the width follows
/// the target and the exported WebAssembly signature is exactly the u32 the
/// JavaScript side sees.
#[cfg(target_arch = "wasm32")]
#[allow(non_camel_case_types)]
pub type WpPtr = u32;
#[cfg(not(target_arch = "wasm32"))]
#[allow(non_camel_case_types)]
pub type WpPtr = usize;

/// What this peer says in its handshake, the shape the mesh sends: the device
/// kind and version, and nothing a browser could not say.
fn hello() -> Vec<u8> {
    format!(
        "{{\"v\":1,\"kind\":\"web\",\"version\":\"{}\"}}",
        env!("CARGO_PKG_VERSION")
    )
    .into_bytes()
}

/// One device's link state on this side.
struct Peer {
    private: [u8; 32],
    link: Option<noise::Link>,
}

/// The module's whole memory that survives between calls: the handles, the
/// buffer the last call returned, and the last error.
struct World {
    next: u32,
    peers: HashMap<u32, Peer>,
    out: Vec<u8>,
    err: i32,
}

fn world() -> &'static Mutex<World> {
    static WORLD: OnceLock<Mutex<World>> = OnceLock::new();
    WORLD.get_or_init(|| {
        Mutex::new(World {
            next: 1,
            peers: HashMap::new(),
            out: Vec::new(),
            err: ERR_NONE,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, World> {
    world()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs one entry point with the boundary's panic guard.
fn guarded(answer: impl FnOnce(&mut World) -> WpPtr) -> WpPtr {
    let mut world = lock();
    world.err = ERR_NONE;
    world.out = Vec::new();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| answer(&mut world))) {
        Ok(ptr) => ptr,
        Err(_) => {
            world.err = ERR_PANIC;
            world.out = Vec::new();
            0
        }
    }
}

/// The bytes at a pointer, for the length the caller promised. Reading is the
/// caller's contract (see each entry point's safety note); a null pointer or an
/// impossible length is not panic material here.
fn bytes<'a>(ptr: WpPtr, len: u32) -> Option<&'a [u8]> {
    if ptr == 0 {
        return None;
    }
    let len = usize::try_from(len).ok()?;
    // SAFETY: every pointer a call is given points into a buffer this module or
    // the caller owns for the duration of the call, as the ABI documents.
    Some(unsafe { std::slice::from_raw_parts(ptr as *const u8, len) })
}

/// A device's bytes at a pointer: exactly 16 of them.
fn device(ptr: WpPtr) -> Result<DeviceId, i32> {
    let raw: [u8; 16] = bytes(ptr, 16)
        .ok_or(ERR_KEY)?
        .try_into()
        .map_err(|_| ERR_KEY)?;
    Ok(DeviceId::from_bytes(raw))
}

/// A frame's type byte, checked against the step that receives it. An empty
/// body is not a frame either.
fn frame_of(frame: &[u8], want: u8) -> Result<&[u8], i32> {
    let (kind, body) = frame.split_first().ok_or(ERR_FRAME)?;
    if *kind == want && !body.is_empty() {
        Ok(body)
    } else {
        Err(ERR_FRAME)
    }
}

/// Store the bytes of the last call and answer their pointer.
fn answer(world: &mut World, data: Vec<u8>) -> WpPtr {
    world.out = data;
    world.out.as_ptr() as WpPtr
}

/// A device for a private key, refusing bytes no X25519 key can be made from.
fn new_peer(private: &[u8]) -> Result<Peer, i32> {
    noise::public_key(private).ok_or(ERR_KEY)?;
    let private: [u8; 32] = private.try_into().map_err(|_| ERR_KEY)?;
    Ok(Peer {
        private,
        link: None,
    })
}

/// One step that needs a handle's private key and a link that is up.
fn with_link(
    world: &mut World,
    handle: u32,
    step: impl FnOnce(&mut noise::Link, &[u8]) -> Result<Vec<u8>, i32>,
    body: &[u8],
) -> Result<Vec<u8>, i32> {
    let Some(peer) = world.peers.get_mut(&handle) else {
        return Err(ERR_STATE);
    };
    let Some(link) = peer.link.as_mut() else {
        return Err(ERR_STATE);
    };
    step(link, body)
}

// ---- the ABI ----

/// A buffer of `len` zeroed bytes for the caller to write into.
///
/// # Safety
/// The caller owns the returned pointer until `wp_free` gives it back.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_alloc(len: u32) -> WpPtr {
    let mut buffer = vec![0u8; len as usize];
    let ptr = buffer.as_mut_ptr() as WpPtr;
    std::mem::forget(buffer);
    ptr
}

/// Gives back a buffer from `wp_alloc`.
///
/// # Safety
/// `ptr` and `len` must be exactly what `wp_alloc` returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_free(ptr: WpPtr, len: u32) {
    if ptr == 0 {
        return;
    }
    // SAFETY: the caller's contract is that this pair came from wp_alloc.
    unsafe {
        drop(Vec::from_raw_parts(
            ptr as *mut u8,
            len as usize,
            len as usize,
        ));
    }
}

/// A handle for a device's private key, or 0 when the 32 bytes are no key.
///
/// # Safety
/// `priv_ptr` must point at 32 readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_new(priv_ptr: WpPtr) -> u32 {
    guarded(|world| {
        let Some(private) = bytes(priv_ptr, 32) else {
            world.err = ERR_KEY;
            return 0;
        };
        match new_peer(private) {
            Ok(peer) => {
                let handle = world.next;
                world.next = world.next.saturating_add(1);
                world.peers.insert(handle, peer);
                handle as WpPtr
            }
            Err(code) => {
                world.err = code;
                0
            }
        }
    }) as u32
}

/// The handle's X25519 public key: 32 bytes, as the registry takes it.
///
/// # Safety
/// `handle` must be one `wp_new` returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_pubkey(handle: u32) -> WpPtr {
    guarded(|world| {
        let private = match world.peers.get(&handle) {
            Some(peer) => peer.private,
            None => {
                world.err = ERR_STATE;
                return 0;
            }
        };
        match noise::public_key(&private) {
            Some(public) => answer(world, public.to_vec()),
            None => {
                world.err = ERR_KEY;
                0
            }
        }
    })
}

/// Starts a link: the HS1 frame to send.
///
/// # Safety
/// `peer_pub` must point at 32 bytes, `account` at `account_len`, and
/// `self_id`/`peer_id` at 16 each.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_initiate(
    handle: u32,
    peer_pub: WpPtr,
    account: WpPtr,
    account_len: u32,
    self_id: WpPtr,
    peer_id: WpPtr,
) -> WpPtr {
    guarded(|world| {
        let private = match world.peers.get(&handle) {
            Some(peer) => peer.private,
            None => {
                world.err = ERR_STATE;
                return 0;
            }
        };
        let (Some(public), Some(account), Ok(me), Ok(peer_id)) = (
            bytes(peer_pub, 32),
            bytes(account, account_len),
            device(self_id),
            device(peer_id),
        ) else {
            world.err = ERR_KEY;
            return 0;
        };
        let Ok(account) = std::str::from_utf8(account) else {
            world.err = ERR_KEY;
            return 0;
        };
        let prologue = noise::prologue(account, me, peer_id);
        match noise::initiate(&private, public, &prologue, &hello()) {
            Ok((link, frame)) => {
                if let Some(peer) = world.peers.get_mut(&handle) {
                    peer.link = Some(link);
                }
                answer(world, frame)
            }
            Err(_) => {
                world.err = ERR_HANDSHAKE;
                0
            }
        }
    })
}

/// Answers an HS1: the HS2 frame to send. The link is up on this side at once.
///
/// # Safety
/// Same pointer rules as `wp_initiate`; `hs1` must point at the frame the peer
/// sent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_respond(
    handle: u32,
    peer_pub: WpPtr,
    account: WpPtr,
    account_len: u32,
    self_id: WpPtr,
    peer_id: WpPtr,
    hs1: WpPtr,
    hs1_len: u32,
) -> WpPtr {
    guarded(|world| {
        let private = match world.peers.get(&handle) {
            Some(peer) => peer.private,
            None => {
                world.err = ERR_STATE;
                return 0;
            }
        };
        let (Some(public), Some(account), Some(frame), Ok(me), Ok(peer_id)) = (
            bytes(peer_pub, 32),
            bytes(account, account_len),
            bytes(hs1, hs1_len),
            device(self_id),
            device(peer_id),
        ) else {
            world.err = ERR_KEY;
            return 0;
        };
        let Ok(account) = std::str::from_utf8(account) else {
            world.err = ERR_KEY;
            return 0;
        };
        let Ok(body) = frame_of(frame, noise::HS1) else {
            world.err = ERR_FRAME;
            return 0;
        };
        let prologue = noise::prologue(account, me, peer_id);
        match noise::respond(&private, public, &prologue, body, &hello()) {
            Ok((link, _their_hello, frame)) => {
                if let Some(peer) = world.peers.get_mut(&handle) {
                    peer.link = Some(link);
                }
                answer(world, frame)
            }
            Err(_) => {
                world.err = ERR_HANDSHAKE;
                0
            }
        }
    })
}

/// Completes an initiated link with the HS2: 0 now up, 1 refused.
///
/// # Safety
/// `hs2` must point at the frame the peer sent.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_complete(handle: u32, hs2: WpPtr, hs2_len: u32) -> i32 {
    guarded(|world| {
        let Some(mut peer) = world.peers.remove(&handle) else {
            world.err = ERR_STATE;
            return 1;
        };
        let Some(frame) = bytes(hs2, hs2_len) else {
            world.err = ERR_FRAME;
            world.peers.insert(handle, peer);
            return 1;
        };
        let Ok(body) = frame_of(frame, noise::HS2) else {
            world.err = ERR_FRAME;
            world.peers.insert(handle, peer);
            return 1;
        };
        let Some(link) = peer.link.take() else {
            world.err = ERR_STATE;
            world.peers.insert(handle, peer);
            return 1;
        };
        match noise::complete(link, body) {
            Ok((link, _their_hello)) => {
                peer.link = Some(link);
                world.peers.insert(handle, peer);
                0
            }
            Err(_) => {
                world.err = ERR_HANDSHAKE;
                world.peers.insert(handle, peer);
                1
            }
        }
    }) as i32
}

/// Seals one message: the transport frame to send (type byte included).
///
/// # Safety
/// `plaintext` must point at `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_seal(handle: u32, plaintext: WpPtr, len: u32) -> WpPtr {
    guarded(|world| {
        let Some(body) = bytes(plaintext, len) else {
            world.err = ERR_FRAME;
            return 0;
        };
        match with_link(
            world,
            handle,
            |link, body| link.seal(body).map_err(|_| ERR_STATE),
            body,
        ) {
            Ok(frame) => answer(world, frame),
            Err(code) => {
                world.err = code;
                0
            }
        }
    })
}

/// Opens one received frame: the plaintext. A frame this link cannot open sets
/// `wp_error` and returns 0, so a dropped frame is never an empty message.
///
/// # Safety
/// `frame` must point at `len` readable bytes, the payload the relay delivered.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wp_open(handle: u32, frame: WpPtr, len: u32) -> WpPtr {
    guarded(|world| {
        let Some(frame) = bytes(frame, len) else {
            world.err = ERR_FRAME;
            return 0;
        };
        let Ok(body) = frame_of(frame, noise::MSG) else {
            world.err = ERR_FRAME;
            return 0;
        };
        match with_link(
            world,
            handle,
            |link, body| link.open(body).map_err(|_| ERR_FRAME),
            body,
        ) {
            Ok(plaintext) => answer(world, plaintext),
            Err(code) => {
                world.err = code;
                0
            }
        }
    })
}

/// The length of the buffer the last call returned.
#[unsafe(no_mangle)]
pub extern "C" fn wp_len() -> u32 {
    lock().out.len() as u32
}

/// Why the last call failed: 0 when it did not.
#[unsafe(no_mangle)]
pub extern "C" fn wp_error() -> i32 {
    lock().err
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_noise as noise;

    /// Calls the ABI the way JavaScript does: bytes in linear memory, the
    /// returned pointer with `wp_len`.
    fn take(answer: WpPtr) -> Vec<u8> {
        let len = wp_len();
        if answer == 0 {
            return Vec::new();
        }
        // SAFETY: a returned pointer with wp_len is one buffer this module owns
        // until the next call.
        unsafe { std::slice::from_raw_parts(answer as *const u8, len as usize).to_vec() }
    }

    /// Puts bytes into linear memory and answers the pointer.
    fn write(bytes: &[u8]) -> WpPtr {
        let ptr = unsafe { wp_alloc(bytes.len() as u32) };
        // SAFETY: wp_alloc just made exactly this much memory.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
        ptr
    }

    /// The module keeps one World (the last answer's buffer and error), as
    /// a browser's single thread does, so tests that read `wp_len` or
    /// `wp_error` after a call must not interleave: each holds this.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    struct Fixture {
        account: Vec<u8>,
        id_a: [u8; 16],
        id_b: [u8; 16],
        a: u32,
        b: u32,
        pub_a: Vec<u8>,
        pub_b: Vec<u8>,
    }

    fn fixture() -> Fixture {
        let account = b"acct-1".to_vec();
        let id_a = DeviceId::new().to_bytes();
        let id_b = DeviceId::new().to_bytes();
        // Fixed keys: the browser mints them with crypto.getRandomValues and
        // hands the bytes over, so the module never needs a random source.
        let a = unsafe { wp_new(write(&[1u8; 32])) };
        let b = unsafe { wp_new(write(&[2u8; 32])) };
        assert_ne!(a, 0, "a usable key mints a handle: error {}", wp_error());
        assert_ne!(b, 0);
        Fixture {
            account,
            id_a,
            id_b,
            a,
            b,
            pub_a: take(unsafe { wp_pubkey(a) }),
            pub_b: take(unsafe { wp_pubkey(b) }),
        }
    }

    /// One full handshake through the ABI, the way the glue drives it.
    fn linked(f: &Fixture) -> (u32, u32) {
        let hs1 = take(unsafe {
            wp_initiate(
                f.a,
                write(&f.pub_b),
                write(&f.account),
                f.account.len() as u32,
                write(&f.id_a),
                write(&f.id_b),
            )
        });
        assert_eq!(hs1.first(), Some(&noise::HS1), "a handshake frame");
        let hs2 = take(unsafe {
            wp_respond(
                f.b,
                write(&f.pub_a),
                write(&f.account),
                f.account.len() as u32,
                write(&f.id_b),
                write(&f.id_a),
                write(&hs1),
                hs1.len() as u32,
            )
        });
        assert_eq!(hs2.first(), Some(&noise::HS2), "a response frame");
        let up = unsafe { wp_complete(f.a, write(&hs2), hs2.len() as u32) };
        assert_eq!(up, 0, "the initiator is up: error {}", wp_error());
        (f.a, f.b)
    }

    #[test]
    fn two_handles_link_and_exchange_a_sealed_message() {
        let _serial = serial();
        let f = fixture();
        let (a, b) = linked(&f);
        let request = b"{\"method\":\"GET\"}";
        let sealed = take(unsafe { wp_seal(a, write(request), request.len() as u32) });
        assert_eq!(sealed.first(), Some(&noise::MSG));
        assert!(
            !sealed.windows(6).any(|w| w == b"method"),
            "the relay sees ciphertext"
        );
        let plain = take(unsafe { wp_open(b, write(&sealed), sealed.len() as u32) });
        assert_eq!(wp_error(), ERR_NONE, "opened, not dropped");
        assert_eq!(plain, request);

        // The same frame again is refused: a replay is not a message.
        assert_eq!(
            unsafe { wp_open(b, write(&sealed), sealed.len() as u32) },
            0
        );
        assert_ne!(wp_error(), ERR_NONE, "a replay says why");
    }

    #[test]
    fn a_wrong_peer_key_or_another_account_never_links() {
        let _serial = serial();
        let f = fixture();
        let stranger = take(unsafe { wp_pubkey(wp_new(write(&[3u8; 32]))) });
        let hs1 = take(unsafe {
            wp_initiate(
                f.a,
                write(&stranger),
                write(&f.account),
                f.account.len() as u32,
                write(&f.id_a),
                write(&f.id_b),
            )
        });
        assert_eq!(hs1.first(), Some(&noise::HS1));
        let refused = unsafe {
            wp_respond(
                f.b,
                write(&f.pub_a),
                write(&f.account),
                f.account.len() as u32,
                write(&f.id_b),
                write(&f.id_a),
                write(&hs1),
                hs1.len() as u32,
            )
        };
        assert_eq!(refused, 0, "a key that is not the peer's is refused");
        assert_eq!(wp_error(), ERR_HANDSHAKE);

        let other = b"acct-2".to_vec();
        let hs1 = take(unsafe {
            wp_initiate(
                f.a,
                write(&f.pub_b),
                write(&other),
                other.len() as u32,
                write(&f.id_a),
                write(&f.id_b),
            )
        });
        let refused = unsafe {
            wp_respond(
                f.b,
                write(&f.pub_a),
                write(&f.account),
                f.account.len() as u32,
                write(&f.id_b),
                write(&f.id_a),
                write(&hs1),
                hs1.len() as u32,
            )
        };
        assert_eq!(refused, 0, "another account's prologue is refused");
        assert_eq!(wp_error(), ERR_HANDSHAKE);
    }

    #[test]
    fn a_blank_or_shortened_private_key_mints_no_device() {
        let _serial = serial();
        assert_eq!(new_peer(&[]).err(), Some(ERR_KEY), "an empty key");
        assert_eq!(new_peer(&[1u8; 16]).err(), Some(ERR_KEY), "a shortened key");
        assert_eq!(new_peer(&[0u8; 32]).err(), Some(ERR_KEY), "all zeros");
        assert!(new_peer(&[7u8; 32]).is_ok());

        assert_eq!(unsafe { wp_new(write(&[0u8; 32])) }, 0);
        assert_eq!(wp_error(), ERR_KEY);
    }

    #[test]
    fn a_frame_of_the_wrong_kind_is_refused_not_opened() {
        let _serial = serial();
        let f = fixture();
        let (_a, b) = linked(&f);
        let mut frame = vec![noise::HS1];
        frame.extend_from_slice(&[9u8; 40]);
        assert_eq!(unsafe { wp_open(b, write(&frame), frame.len() as u32) }, 0);
        assert_eq!(wp_error(), ERR_FRAME);
        assert_eq!(unsafe { wp_open(b, write(&[noise::MSG]), 1) }, 0);
        assert_eq!(wp_error(), ERR_FRAME);
    }
}
