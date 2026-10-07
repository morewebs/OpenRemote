//! One end-to-end encrypted link between two of an account's devices:
//! Noise KK (both sides already know each other's static key from the
//! registry), over relay frames. The relay sees only these bytes.
//!
//! Relay payloads start with a type byte: 1 the initiator's handshake
//! message, 2 the responder's, 3 a transport message, 4 a nudge (the
//! higher id asking the lower to initiate, so the two never initiate at
//! once).

use openremote_core::DeviceId;

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

pub enum Link {
    /// Sent HS1, waiting for HS2.
    Initiating(Box<snow::HandshakeState>),
    Up(Box<snow::TransportState>),
}

fn builder<'a>(
    private: &'a [u8],
    remote: &'a [u8],
    prologue: &'a [u8],
) -> Result<snow::Builder<'a>, snow::Error> {
    snow::Builder::new(crate::identity::NOISE_PARAMS.parse()?)
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

    fn keys() -> snow::Keypair {
        snow::Builder::new(crate::identity::NOISE_PARAMS.parse().unwrap())
            .generate_keypair()
            .unwrap()
    }

    #[test]
    fn two_devices_link_and_talk_only_with_the_right_keys_and_prologue() {
        let (a, b) = (keys(), keys());
        let (ida, idb) = (DeviceId::new(), DeviceId::new());
        let p = prologue("acct", ida, idb);
        assert_eq!(p, prologue("acct", idb, ida), "both sides agree");

        let (link_a, hs1) = initiate(&a.private, &b.public, &p, b"hi from a").unwrap();
        let (mut up_b, hello_a, hs2) =
            respond(&b.private, &a.public, &p, &hs1[1..], b"hi from b").unwrap();
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
        let stranger = keys();
        let (_, hs1) = initiate(&stranger.private, &b.public, &p, b"").unwrap();
        assert!(respond(&b.private, &a.public, &p, &hs1[1..], b"").is_err());
        let (_, hs1) = initiate(&a.private, &b.public, &prologue("other", ida, idb), b"").unwrap();
        assert!(respond(&b.private, &a.public, &p, &hs1[1..], b"").is_err());
        assert!(initiates(ida, idb) != initiates(idb, ida));
    }
}
