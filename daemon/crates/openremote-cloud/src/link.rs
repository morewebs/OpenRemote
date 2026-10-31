//! One end-to-end encrypted link between two of an account's devices: Noise KK
//! (both sides already know each other's static key from the registry), over
//! relay frames. The relay sees only these bytes.
//!
//! The implementation moved to `openremote-noise` so the browser's web peer can
//! compile the same protocol to WebAssembly; this module keeps the path every
//! caller already uses, re-exported unchanged, so there is still exactly one
//! implementation of the link.

pub use openremote_noise::*;
