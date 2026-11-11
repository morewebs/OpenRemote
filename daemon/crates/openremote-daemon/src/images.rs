//! Pictures that ride a chat: the composer's attachments arriving with a
//! message, and the pictures a tool answers with (an agent's screenshot of
//! a page). Checked, hashed, and kept in the blob store under the chat
//! that brought them; the events only name them.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use openremote_core::blobs::Blobs;
use openremote_core::{ImageRef, SessionId};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One picture a message may carry, at most: the Messages API's own limit
/// for an image block, so what the composer sends claude can read.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

/// Pictures per message, at most.
pub const MAX_IMAGES: usize = 10;

/// A tool's picture is the agent's own doing (a full-page screenshot can
/// be large): kept up to this, dropped past it - the result's words still
/// say what happened.
pub const MAX_TOOL_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// The body a message's routes take (prompts, the queue, steer): every
/// picture at its limit as base64, and room for the words.
pub const MESSAGE_BODY_LIMIT: usize = 72 * 1024 * 1024;

/// A picture as a message's body carries it.
#[derive(Deserialize, Clone, Debug)]
pub struct Attachment {
    /// `image`, the one kind there is today.
    pub kind: String,
    /// What the sender says it is. Never trusted: the bytes say.
    #[serde(default)]
    pub mime: String,
    /// The bytes in base64 (a whole `data:` URL is taken too).
    #[serde(default)]
    pub data: String,
}

/// A picture checked and ready to keep.
#[derive(Clone, Debug)]
pub struct NewImage {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

/// A picture's type from its first bytes: the four kinds the harnesses
/// read (PNG, JPEG, GIF, WebP), and nothing else - an SVG can carry
/// script, and a claude turn fails outright on a picture whose bytes don't
/// match the type it was sent as.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// A message's attachments as pictures, or the reason they can't go, in
/// words the composer shows as they are.
pub fn decode(attachments: &[Attachment]) -> Result<Vec<NewImage>, String> {
    if attachments.len() > MAX_IMAGES {
        return Err(format!(
            "A message can carry {MAX_IMAGES} pictures at most."
        ));
    }
    attachments
        .iter()
        .enumerate()
        .map(|(i, attachment)| {
            let n = i + 1;
            if attachment.kind != "image" {
                return Err(format!(
                    "Attachment {n} is a {}, and only pictures can be sent.",
                    attachment.kind
                ));
            }
            let data = attachment
                .data
                .split_once(";base64,")
                .map(|(_, data)| data)
                .unwrap_or(&attachment.data);
            let bytes = STANDARD
                .decode(data.trim())
                .map_err(|_| format!("Picture {n} didn't arrive whole - attach it again."))?;
            if bytes.len() > MAX_IMAGE_BYTES {
                return Err(format!("Picture {n} is over 5 MB - send a smaller one."));
            }
            let mime = sniff(&bytes)
                .ok_or_else(|| format!("Picture {n} isn't a PNG, JPEG, GIF or WebP."))?;
            Ok(NewImage { mime, bytes })
        })
        .collect()
}

/// The sha256 a picture is kept under, in lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keep a message's pictures for a chat: what its events name them by.
pub fn keep(
    blobs: &Blobs,
    owner: SessionId,
    images: &[NewImage],
) -> std::io::Result<Vec<ImageRef>> {
    images
        .iter()
        .map(|image| {
            let sha = sha256_hex(&image.bytes);
            blobs.put(&sha, image.mime, &image.bytes, owner)?;
            Ok(ImageRef {
                blob: sha,
                mime: image.mime.to_string(),
            })
        })
        .collect()
}

/// A tool result's pictures, kept for the chat. One that doesn't decode,
/// isn't a picture, or is too large is left out rather than fail the
/// result: its words still arrive.
pub fn keep_tool_images(
    blobs: &Blobs,
    owner: SessionId,
    images: &[openremote_harness::Image],
) -> Vec<ImageRef> {
    images
        .iter()
        .filter_map(|image| {
            let bytes = STANDARD.decode(image.data.trim()).ok()?;
            if bytes.len() > MAX_TOOL_IMAGE_BYTES {
                return None;
            }
            let mime = sniff(&bytes)?;
            let sha = sha256_hex(&bytes);
            match blobs.put(&sha, mime, &bytes, owner) {
                Ok(()) => Some(ImageRef {
                    blob: sha,
                    mime: mime.to_string(),
                }),
                Err(e) => {
                    eprintln!("session {owner}: a tool's picture wasn't kept: {e}");
                    None
                }
            }
        })
        .collect()
}

/// A kept picture as a harness's wire wants it: for a harness that reads
/// files, the picture as a file named with its extension; for the others,
/// its bytes in base64.
pub fn for_harness(
    blobs: &Blobs,
    image: &ImageRef,
    as_file: bool,
) -> std::io::Result<openremote_harness::Image> {
    let gone = || {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "a picture this message carried is no longer kept",
        )
    };
    if as_file {
        // The named file comes from the kept bytes: no bytes, no file.
        let path = blobs.file(&image.blob, &image.mime).map_err(|_| gone())?;
        return Ok(openremote_harness::Image {
            mime: image.mime.clone(),
            data: String::new(),
            path: Some(path),
        });
    }
    let Some((_, bytes)) = blobs.get(&image.blob)? else {
        return Err(gone());
    };
    Ok(openremote_harness::Image {
        mime: image.mime.clone(),
        data: STANDARD.encode(bytes),
        path: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest of a png";

    fn attachment(bytes: &[u8]) -> Attachment {
        Attachment {
            kind: "image".into(),
            mime: "image/png".into(),
            data: STANDARD.encode(bytes),
        }
    }

    #[test]
    fn a_picture_is_typed_by_its_own_bytes() {
        assert_eq!(sniff(PNG), Some("image/png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<svg onload=alert(1)>"), None);
        // The sender's word is never trusted: a PNG sent as a JPEG is a PNG.
        let mut lying = attachment(PNG);
        lying.mime = "image/jpeg".into();
        assert_eq!(decode(&[lying]).unwrap()[0].mime, "image/png");
    }

    #[test]
    fn a_data_url_is_taken_whole() {
        let mut url = attachment(PNG);
        url.data = format!("data:image/png;base64,{}", url.data);
        assert_eq!(decode(&[url]).unwrap()[0].bytes, PNG);
    }

    #[test]
    fn the_limits_refuse_in_words_the_composer_shows() {
        let mut big = PNG.to_vec();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        assert_eq!(
            decode(&[attachment(&big)]).unwrap_err(),
            "Picture 1 is over 5 MB - send a smaller one."
        );
        let mut fits = PNG.to_vec();
        fits.resize(MAX_IMAGE_BYTES, 0);
        assert!(decode(&[attachment(&fits)]).is_ok());

        let eleven = vec![attachment(PNG); MAX_IMAGES + 1];
        assert!(decode(&eleven).unwrap_err().contains("10 pictures at most"));
        assert!(
            decode(&[attachment(b"plain words")])
                .unwrap_err()
                .contains("isn't a PNG, JPEG, GIF or WebP")
        );
        let mut file = attachment(PNG);
        file.kind = "file".into();
        assert!(decode(&[file]).unwrap_err().contains("only pictures"));
    }

    #[test]
    fn a_kept_picture_is_named_by_its_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = Blobs::new(dir.path());
        let chat = SessionId::new();
        let kept = keep(&blobs, chat, &decode(&[attachment(PNG)]).unwrap()).unwrap();
        assert_eq!(kept[0].blob, sha256_hex(PNG));
        assert_eq!(kept[0].mime, "image/png");
        assert!(blobs.held_by(&kept[0].blob, chat));
        let wire = for_harness(&blobs, &kept[0], false).unwrap();
        assert_eq!(wire.data, STANDARD.encode(PNG));
        let file = for_harness(&blobs, &kept[0], true).unwrap();
        assert_eq!(std::fs::read(file.path.unwrap()).unwrap(), PNG);
    }
}
