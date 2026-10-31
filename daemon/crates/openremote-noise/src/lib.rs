//! One end-to-end encrypted link between two of an account's devices: Noise KK
//! (both sides already know each other's static key from the registry), over
//! relay frames. The relay sees only these bytes.
//!
//! Relay payloads start with a type byte: 1 the initiator's handshake message,
//! 2 the responder's, 3 a transport message, 4 a nudge (the higher id asking
//! the lower to initiate, so the two never initiate at once).
//!
//! The daemons and the browser's web peer share this module: the protocol is
//! written once, in Rust, and the browser compiles it to WebAssembly instead of
//! reimplementing Noise in JavaScript, where a single mismatch would look like
//! an intermittent network fault.

use openremote_core::DeviceId;

/// The one Noise handshake this system speaks: static-static, with the
/// account and both device ids bound in the prologue.
pub const NOISE_PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";

pub const HS1: u8 = 1;
pub const HS2: u8 = 2;
pub const MSG: u8 = 3;
pub const NUDGE: u8 = 4;

/// A Noise message is at most 65535 bytes; the tag takes 16.
pub const MAX_PLAINTEXT: usize = 65535 - 16;

/// Binds a link to the account and to exactly these two devices, so a
/// handshake can't be replayed between other devices or accounts.
pub fn prologue(account: &str, a: DeviceId, b: DeviceId) -> Vec<u8> {
    let (lo, hi) = if a.to_bytes() <= b.to_bytes() {
        (a, b)
    } else {
        (b, a)
    };
    let mut out = b"openremote-mesh/1\0".to_vec();
    out.extend_from_slice(account.as_bytes());
    out.push(0);
    out.extend_from_slice(&lo.to_bytes());
    out.extend_from_slice(&hi.to_bytes());
    out
}

/// The lower id initiates; the higher one nudges.
pub fn initiates(me: DeviceId, peer: DeviceId) -> bool {
    me.to_bytes() < peer.to_bytes()
}

/// The X25519 public key for a private one, or None when the bytes are not a
/// usable key: an empty or shortened key, or all zeros, which clamps to the
/// identity point and can never complete a handshake.
///
/// The same curve snow uses, so the key the registry is told is the key the
/// handshake will use.
pub fn public_key(private: &[u8]) -> Option<[u8; 32]> {
    let bytes: [u8; 32] = private.try_into().ok()?;
    if bytes.iter().all(|byte| *byte == 0) {
        return None;
    }
    let point = curve25519_dalek::montgomery::MontgomeryPoint::mul_base_clamped(bytes);
    Some(point.0)
}

pub enum Link {
    /// Sent HS1, waiting for HS2.
    Initiating(Box<snow::HandshakeState>),
    Up(Box<snow::TransportState>),
}

/// The browser's own random source, supplied by the JavaScript glue at
/// instantiation. A WebAssembly module has no operating system to ask for an
/// ephemeral key, and the page has `crypto.getRandomValues`.
#[cfg(target_arch = "wasm32")]
mod browser_entropy {
    use snow::params::{CipherChoice, DHChoice, HashChoice};
    use snow::resolvers::{CryptoResolver, DefaultResolver};
    use snow::types::{Cipher, Dh, Hash, Random};

    unsafe extern "C" {
        /// Fills `len` bytes with the page's cryptographic random bytes. The
        /// glue's implementation is `crypto.getRandomValues`.
        fn wp_random_fill(ptr: *mut u8, len: usize);
    }

    struct PageRandom;

    impl Random for PageRandom {
        fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), snow::Error> {
            // SAFETY: the import writes exactly dest.len() bytes into dest.
            unsafe { wp_random_fill(dest.as_mut_ptr(), dest.len()) };
            Ok(())
        }
    }

    /// snow's resolver with one difference: the random bytes come from the
    /// page. The X25519, ChaChaPoly and BLAKE2s implementations are the ones
    /// the daemons run, which is what keeps the two ends byte for byte.
    pub(super) struct BrowserResolver(pub(super) DefaultResolver);

    impl CryptoResolver for BrowserResolver {
        fn resolve_rng(&self) -> Option<Box<dyn Random>> {
            Some(Box::new(PageRandom))
        }

        fn resolve_dh(&self, choice: &DHChoice) -> Option<Box<dyn Dh>> {
            self.0.resolve_dh(choice)
        }

        fn resolve_hash(&self, choice: &HashChoice) -> Option<Box<dyn Hash>> {
            self.0.resolve_hash(choice)
        }

        fn resolve_cipher(&self, choice: &CipherChoice) -> Option<Box<dyn Cipher>> {
            self.0.resolve_cipher(choice)
        }
    }
}

fn builder<'a>(
    private: &'a [u8],
    remote: &'a [u8],
    prologue: &'a [u8],
) -> Result<snow::Builder<'a>, snow::Error> {
    // The handshake needs one ephemeral key, so both builds need a random
    // source; only where it comes from differs.
    #[cfg(target_arch = "wasm32")]
    let builder = snow::Builder::with_resolver(
        NOISE_PARAMS.parse()?,
        Box::new(browser_entropy::BrowserResolver(Default::default())),
    );
    #[cfg(not(target_arch = "wasm32"))]
    let builder = snow::Builder::new(NOISE_PARAMS.parse()?);
    builder
        .local_private_key(private)?
        .remote_public_key(remote)?
        .prologue(prologue)
}

/// Starts a link: the HS1 payload to send (type byte included).
pub fn initiate(
    private: &[u8],
    remote: &[u8],
    prologue: &[u8],
    hello: &[u8],
) -> Result<(Link, Vec<u8>), snow::Error> {
    let mut hs = builder(private, remote, prologue)?.build_initiator()?;
    let mut out = vec![0u8; 65535];
    let n = hs.write_message(hello, &mut out)?;
    let mut frame = vec![HS1];
    frame.extend_from_slice(&out[..n]);
    Ok((Link::Initiating(Box::new(hs)), frame))
}

/// Answers an HS1: the link is up at once on this side. Returns the
/// initiator's hello and the HS2 payload to send.
pub fn respond(
    private: &[u8],
    remote: &[u8],
    prologue: &[u8],
    hs1: &[u8],
    hello: &[u8],
) -> Result<(Link, Vec<u8>, Vec<u8>), snow::Error> {
    let mut hs = builder(private, remote, prologue)?.build_responder()?;
    let mut their_hello = vec![0u8; 65535];
    let n = hs.read_message(hs1, &mut their_hello)?;
    their_hello.truncate(n);
    let mut out = vec![0u8; 65535];
    let m = hs.write_message(hello, &mut out)?;
    let mut frame = vec![HS2];
    frame.extend_from_slice(&out[..m]);
    Ok((
        Link::Up(Box::new(hs.into_transport_mode()?)),
        their_hello,
        frame,
    ))
}

/// Completes an initiated link with the HS2; returns the responder's hello.
pub fn complete(link: Link, hs2: &[u8]) -> Result<(Link, Vec<u8>), snow::Error> {
    let Link::Initiating(mut hs) = link else {
        return Err(snow::Error::State(
            snow::error::StateProblem::HandshakeAlreadyFinished,
        ));
    };
    let mut hello = vec![0u8; 65535];
    let n = hs.read_message(hs2, &mut hello)?;
    hello.truncate(n);
    Ok((Link::Up(Box::new(hs.into_transport_mode()?)), hello))
}

impl Link {
    pub fn is_up(&self) -> bool {
        matches!(self, Link::Up(_))
    }

    /// One transport message (type byte included).
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, snow::Error> {
        let Link::Up(t) = self else {
            return Err(snow::Error::State(
                snow::error::StateProblem::HandshakeNotFinished,
            ));
        };
        let mut out = vec![0u8; plaintext.len() + 16 + 1];
        out[0] = MSG;
        let n = t.write_message(plaintext, &mut out[1..])?;
        out.truncate(n + 1);
        Ok(out)
    }

    pub fn open(&mut self, message: &[u8]) -> Result<Vec<u8>, snow::Error> {
        let Link::Up(t) = self else {
            return Err(snow::Error::State(
                snow::error::StateProblem::HandshakeNotFinished,
            ));
        };
        let mut out = vec![0u8; message.len()];
        let n = t.read_message(message, &mut out)?;
        out.truncate(n);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed private keys: the browser hands this module raw key bytes and
    /// never asks it to generate one, so the test needs no random source.
    fn keys(seed: u8) -> ([u8; 32], [u8; 32]) {
        let private = [seed; 32];
        let public = public_key(&private).expect("a usable key");
        (private, public)
    }

    #[test]
    fn two_devices_link_and_talk_only_with_the_right_keys_and_prologue() {
        let (a, b) = (keys(1), keys(2));
        let (ida, idb) = (DeviceId::new(), DeviceId::new());
        let p = prologue("acct", ida, idb);
        assert_eq!(p, prologue("acct", idb, ida), "both sides agree");

        let (link_a, hs1) = initiate(&a.0, &b.1, &p, b"hi from a").unwrap();
        let (mut up_b, hello_a, hs2) = respond(&b.0, &a.1, &p, &hs1[1..], b"hi from b").unwrap();
        assert_eq!(hello_a, b"hi from a");
        let (mut up_a, hello_b) = complete(link_a, &hs2[1..]).unwrap();
        assert_eq!(hello_b, b"hi from b");

        let sealed = up_a.seal(b"ciphertext only").unwrap();
        assert_eq!(sealed[0], MSG);
        assert!(!sealed.windows(10).any(|w| w == b"ciphertext"));
        assert_eq!(up_b.open(&sealed[1..]).unwrap(), b"ciphertext only");
        let back = up_b.seal(b"and back").unwrap();
        assert_eq!(up_a.open(&back[1..]).unwrap(), b"and back");
        assert!(up_a.open(&back[1..]).is_err(), "a replayed message fails");

        // The wrong key, or another account's prologue, never links.
        let stranger = keys(3);
        let (_, hs1) = initiate(&stranger.0, &b.1, &p, b"").unwrap();
        assert!(respond(&b.0, &a.1, &p, &hs1[1..], b"").is_err());
        let (_, hs1) = initiate(&a.0, &b.1, &prologue("other", ida, idb), b"").unwrap();
        assert!(respond(&b.0, &a.1, &p, &hs1[1..], b"").is_err());
        assert!(initiates(ida, idb) != initiates(idb, ida));
    }

    #[test]
    fn a_private_key_says_its_public_key_and_a_blank_one_is_refused() {
        let (private, public) = keys(9);
        assert_eq!(public_key(&private), Some(public));
        assert_eq!(public_key(&[0u8; 32]), None, "all zeros is not a key");
        assert_eq!(public_key(&[]), None, "an empty key is not a key");
        assert_eq!(public_key(&[1u8; 16]), None, "a shortened key is not a key");
    }
}
