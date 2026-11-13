//! The bytes an in-app update travels as.
//!
//! Three things are zstd here. A whole installer, packed for download or for
//! the copy the app keeps. And a patch that turns one release's installer into
//! the next, which is zstd with the old installer as its dictionary: every byte
//! the new one shares with the old is a reference, and only the rest is data.
//!
//! The installer a patch is made from is built uncompressed. An LZMA installer
//! scrambles a small change into a change to most of the file, so two LZMA
//! installers differ nearly everywhere; uncompressed ones differ where the
//! change is. The download is packed separately, so it stays small too.
//!
//! The app only applies patches. The release tool (scripts/delta-tool) makes
//! them, and includes this file itself, so the format has one definition.

use std::io::{self, BufReader, Read, Write};

/// The largest installer this codec will produce or accept. A real installer is
/// tens of megabytes; the cap stops a damaged stream from filling memory.
pub const MAX_INSTALLER_BYTES: u64 = 1 << 30;

/// The window a patch is made and applied with, as a power of two: 128 MiB.
/// It must reach back across the whole base. zstd's default window is a few
/// megabytes, which would miss most of an installer.
const WINDOW_LOG: u32 = 27;

/// The size of the match tables a patch is made with, as a power of two.
const TABLE_LOG: u32 = 26;

/// The level a download is packed at. A release packs it once, on its build
/// machine, so the size is worth the time.
#[allow(dead_code)] // the release tool packs at this level; the app does not
pub const PACK_LEVEL: i32 = 19;

/// The level the app keeps its base at. The app packs it while an update
/// installs, so it favours speed; the base is read back exactly either way.
#[allow(dead_code)] // the app keeps its base at this level; the release tool does not
pub const STORE_LEVEL: i32 = 3;

/// Packs one installer.
pub fn pack(raw: &[u8], level: i32) -> io::Result<Vec<u8>> {
    zstd::stream::encode_all(raw, level)
}

/// Unpacks a downloaded or stored installer.
pub fn unpack(packed: &[u8]) -> io::Result<Vec<u8>> {
    read_capped(zstd::stream::Decoder::new(packed)?)
}

/// Makes the patch from `base` (the old uncompressed installer) to `new`.
/// Only the release tool calls this; the app just applies patches.
#[allow(dead_code)] // the release tool makes patches; the app only applies them
pub fn make_patch(base: &[u8], new: &[u8]) -> io::Result<Vec<u8>> {
    let mut encoder = zstd::stream::Encoder::with_dictionary(Vec::new(), PACK_LEVEL, base)?;
    encoder.window_log(WINDOW_LOG)?;
    encoder.long_distance_matching(true)?;
    // The base is tens of megabytes and the default match tables index only a
    // fraction of it, so most of the base would be sent as literals. Tables sized
    // for the whole base keep its positions to match against.
    encoder.set_parameter(zstd::zstd_safe::CParameter::HashLog(TABLE_LOG))?;
    encoder.set_parameter(zstd::zstd_safe::CParameter::ChainLog(TABLE_LOG + 1))?;
    encoder.write_all(new)?;
    encoder.finish()
}

/// Applies a patch to `base`, giving the new uncompressed installer. A patch
/// applied to the wrong base gives an error or other bytes, never the new
/// installer; the caller checks the result against its signature, which is
/// what makes it the right one.
pub fn apply_patch(base: &[u8], patch: &[u8]) -> io::Result<Vec<u8>> {
    let mut decoder = zstd::stream::Decoder::with_dictionary(BufReader::new(patch), base)?;
    decoder.window_log_max(WINDOW_LOG)?;
    read_capped(decoder)
}

fn read_capped<R: Read>(reader: R) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.take(MAX_INSTALLER_BYTES + 1).read_to_end(&mut out)?;
    if out.len() as u64 > MAX_INSTALLER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the installer is larger than an update may be",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{apply_patch, make_patch, pack, unpack, PACK_LEVEL};

    /// A stand-in installer: a few hundred kilobytes of varied bytes.
    fn installer(seed: u32) -> Vec<u8> {
        let mut state = seed;
        (0..400_000)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (state >> 16) as u8
            })
            .collect()
    }

    #[test]
    fn a_packed_installer_unpacks_to_the_same_bytes() {
        let raw = installer(1);
        let packed = pack(&raw, PACK_LEVEL).expect("pack");
        assert_eq!(unpack(&packed).expect("unpack"), raw);
    }

    #[test]
    fn a_patch_turns_the_old_installer_into_the_new_one() {
        let old = installer(7);
        let mut new = old.clone();
        // One change in the middle, the way a rebuilt daemon changes its file.
        new[200_000..200_064].copy_from_slice(&[0xAB; 64]);
        let patch = make_patch(&old, &new).expect("make");
        assert_eq!(apply_patch(&old, &patch).expect("apply"), new);
        assert!(
            patch.len() < 4_000,
            "a 64-byte change is a few kilobytes of patch, not the installer ({} bytes)",
            patch.len()
        );
    }

    #[test]
    fn a_patch_applied_to_another_base_is_not_the_new_installer() {
        let old = installer(7);
        let new = {
            let mut n = old.clone();
            n[10..20].copy_from_slice(&[1; 10]);
            n
        };
        let patch = make_patch(&old, &new).expect("make");
        // Either an error or other bytes: never the new installer.
        if let Ok(bytes) = apply_patch(&installer(8), &patch) {
            assert_ne!(bytes, new, "a wrong base must not reproduce the installer");
        }
    }

    #[test]
    fn garbage_is_refused_as_a_patch() {
        assert!(apply_patch(&installer(1), b"not a patch at all").is_err());
        assert!(unpack(b"not zstd").is_err());
    }
}
