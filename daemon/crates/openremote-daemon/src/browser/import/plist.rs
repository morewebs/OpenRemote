//! A reader for binary property lists (`bplist00`), enough for Safari's
//! bookmarks: strings, numbers, arrays and dictionaries; everything else
//! reads as `Other`. The format is Apple's, public in its open-source
//! CoreFoundation (`CFBinaryPList.c`).
//!
//! The bytes are someone else's. Every offset is checked, no arithmetic can
//! overflow, nesting stops at [`MAX_DEPTH`], and the whole list may expand
//! to no more than [`MAX_OBJECTS`] objects: a list whose arrays all point at
//! one shared array would otherwise unfold into billions of copies.

use std::cell::Cell;

#[derive(Clone, Debug, PartialEq)]
pub enum Plist {
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    Data(Vec<u8>),
    Array(Vec<Plist>),
    Dict(Vec<(String, Plist)>),
    Other,
}

impl Plist {
    pub fn get(&self, key: &str) -> Option<&Plist> {
        match self {
            Plist::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Plist::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn array(&self) -> Option<&[Plist]> {
        match self {
            Plist::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// A list nested deeper than this is damaged (or a cycle).
const MAX_DEPTH: u32 = 64;
/// The most objects one list may unfold into. A large Safari bookmarks file
/// is tens of thousands.
pub const MAX_OBJECTS: usize = 500_000;

struct Reader<'a> {
    bytes: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: usize,
    table: usize,
    /// Objects still allowed before the list is refused.
    budget: Cell<usize>,
}

/// The list's top object, or none when the bytes aren't a binary list (or
/// are a damaged one).
pub fn parse(bytes: &[u8]) -> Option<Plist> {
    if bytes.len() < 8 + 32 || !bytes.starts_with(b"bplist00") {
        return None;
    }
    let trailer = &bytes[bytes.len() - 32..];
    let be = |b: &[u8]| -> Option<usize> {
        b.iter().try_fold(0usize, |n, byte| {
            n.checked_mul(256)?.checked_add(*byte as usize)
        })
    };
    let reader = Reader {
        bytes,
        offset_size: trailer[6] as usize,
        ref_size: trailer[7] as usize,
        count: be(&trailer[8..16])?,
        table: be(&trailer[24..32])?,
        budget: Cell::new(MAX_OBJECTS),
    };
    if !(1..=8).contains(&reader.offset_size) || !(1..=8).contains(&reader.ref_size) {
        return None;
    }
    reader.object(be(&trailer[16..24])?, 0)
}

impl Reader<'_> {
    /// A big-endian unsigned number of `size` bytes (at most eight) at `at`.
    fn number(&self, at: usize, size: usize) -> Option<usize> {
        if size > 8 {
            return None;
        }
        let bytes = self.bytes.get(at..at.checked_add(size)?)?;
        bytes
            .iter()
            .fold(0u64, |n, b| (n << 8) | *b as u64)
            .try_into()
            .ok()
    }

    /// The `i`th reference in a list of them starting at `from`.
    fn reference(&self, from: usize, i: usize) -> Option<usize> {
        self.number(
            from.checked_add(i.checked_mul(self.ref_size)?)?,
            self.ref_size,
        )
    }

    fn object(&self, index: usize, depth: u32) -> Option<Plist> {
        if depth > MAX_DEPTH || index >= self.count {
            return None;
        }
        let left = self.budget.get().checked_sub(1)?;
        self.budget.set(left);
        let entry = self
            .table
            .checked_add(index.checked_mul(self.offset_size)?)?;
        let at = self.number(entry, self.offset_size)?;
        let marker = *self.bytes.get(at)?;
        let (kind, info) = (marker >> 4, (marker & 0xf) as usize);
        // A length: in the marker, or an integer object after it.
        let sized = |from: usize| -> Option<(usize, usize)> {
            if info != 0xf {
                return Some((info, from));
            }
            let int = *self.bytes.get(from)?;
            if int >> 4 != 1 {
                return None;
            }
            let width = 1usize << (int & 0xf);
            let start = from.checked_add(1)?;
            Some((self.number(start, width)?, start.checked_add(width)?))
        };
        let slice = |from: usize, len: usize| self.bytes.get(from..from.checked_add(len)?);
        Some(match kind {
            0x0 => match marker {
                0x08 => Plist::Bool(false),
                0x09 => Plist::Bool(true),
                _ => Plist::Other,
            },
            0x1 => {
                let width = 1usize << info;
                if width > 8 {
                    return Some(Plist::Other);
                }
                let bytes = slice(at + 1, width)?;
                // Eight-byte integers are signed (the cast keeps their
                // bits); the smaller ones are not.
                Plist::Int(bytes.iter().fold(0u64, |n, b| (n << 8) | *b as u64) as i64)
            }
            0x2 => {
                let bytes = slice(at + 1, 1usize << info)?;
                match bytes.len() {
                    4 => Plist::Real(f32::from_be_bytes(bytes.try_into().ok()?) as f64),
                    8 => Plist::Real(f64::from_be_bytes(bytes.try_into().ok()?)),
                    _ => Plist::Other,
                }
            }
            0x4 => {
                let (len, from) = sized(at + 1)?;
                Plist::Data(slice(from, len)?.to_vec())
            }
            0x5 => {
                let (len, from) = sized(at + 1)?;
                Plist::Text(String::from_utf8_lossy(slice(from, len)?).into_owned())
            }
            0x6 => {
                let (len, from) = sized(at + 1)?;
                let bytes = slice(from, len.checked_mul(2)?)?;
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                Plist::Text(String::from_utf16_lossy(&units))
            }
            0xa => {
                let (len, from) = sized(at + 1)?;
                let mut items = Vec::with_capacity(len.min(4096));
                for i in 0..len {
                    items.push(self.object(self.reference(from, i)?, depth + 1)?);
                }
                Plist::Array(items)
            }
            0xd => {
                let (len, from) = sized(at + 1)?;
                let mut entries = Vec::with_capacity(len.min(4096));
                for i in 0..len {
                    let k = self.reference(from, i)?;
                    let v = self.reference(from, len.checked_add(i)?)?;
                    let Plist::Text(key) = self.object(k, depth + 1)? else {
                        return None;
                    };
                    entries.push((key, self.object(v, depth + 1)?));
                }
                Plist::Dict(entries)
            }
            _ => Plist::Other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/browser-import/safari/Bookmarks.plist"),
        )
        .unwrap()
    }

    #[test]
    fn what_is_not_a_binary_list_is_none() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"<?xml version=\"1.0\"?><plist></plist>"), None);
        assert_eq!(parse(&[0u8; 80]), None);
    }

    #[test]
    fn safaris_bookmarks_read_as_a_tree() {
        let top = parse(&fixture()).unwrap();
        let children = top.get("Children").and_then(Plist::array).unwrap();
        let titles: Vec<&str> = children
            .iter()
            .filter_map(|c| c.get("Title").and_then(Plist::text))
            .collect();
        assert_eq!(
            titles,
            [
                "History",
                "BookmarksBar",
                "BookmarksMenu",
                "com.apple.ReadingList"
            ]
        );
    }

    /// Forty levels of arrays, each holding the next one eight times: tiny on
    /// disk, 8^40 objects unfolded. The budget refuses it at once.
    #[test]
    fn a_list_that_unfolds_without_end_is_refused() {
        let levels = 40usize;
        let mut objects: Vec<Vec<u8>> = Vec::new();
        for level in 0..levels {
            let next = (level + 1).min(levels - 1) as u8;
            let mut array = vec![0xa8];
            array.extend(std::iter::repeat_n(next, 8));
            objects.push(array);
        }
        let mut bytes = b"bplist00".to_vec();
        let mut offsets = Vec::new();
        for object in &objects {
            offsets.push(bytes.len() as u8);
            bytes.extend(object);
        }
        let table = bytes.len();
        bytes.extend(&offsets);
        let mut trailer = vec![0u8; 32];
        trailer[6] = 1;
        trailer[7] = 1;
        trailer[8..16].copy_from_slice(&(levels as u64).to_be_bytes());
        trailer[24..32].copy_from_slice(&(table as u64).to_be_bytes());
        bytes.extend(trailer);
        let started = std::time::Instant::now();
        assert_eq!(parse(&bytes), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn a_damaged_list_never_panics() {
        let clean = fixture();
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..2000 {
            let mut bytes = clean.clone();
            for _ in 0..1 + (next() % 8) {
                let at = 8 + (next() as usize) % (bytes.len() - 8);
                bytes[at] = next() as u8;
            }
            if round % 20 == 0 {
                bytes.truncate((next() as usize) % bytes.len());
            }
            let _ = parse(&bytes);
        }
    }
}
