//! A small read-only reader for SQLite files, enough for the tables other
//! browsers keep their history and bookmarks in: whole-table scans of
//! ordinary (rowid) tables, text in UTF-8, and a write-ahead log applied on
//! top. It never writes, and the file it is given is always the import's own
//! private copy, never the browser's.
//!
//! Why not SQLite itself: that would be C compiled into every build of the
//! daemon (it also ships as a Linux machine binary and inside the Android
//! app) for a feature used once in a while, and no SQLite crate is in the
//! dependency tree today. The format is stable and documented
//! (<https://sqlite.org/fileformat2.html>); only the parts these tables need
//! are here.
//!
//! The file is someone else's, so every number read from it is checked
//! before it is used: no read outside a page, no page read twice in one walk
//! (a damaged tree that points back at itself ends instead of looping), no
//! row larger than [`MAX_PAYLOAD`], and no arithmetic that can overflow. A
//! damaged row is skipped; a damaged tree is an error, never a panic.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::rc::Rc;

/// What a column holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            // A real outside i64's range saturates (`as` does that), it
            // never wraps or panics.
            Value::Real(f) if f.is_finite() => Some(*f as i64),
            _ => None,
        }
    }

    pub fn real(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Real(f) if f.is_finite() => Some(*f),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// The file is not a database this reader understands, or it is damaged.
    Format(&'static str),
    /// No such table (or column).
    Missing(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Format(why) => write!(f, "it is not a database that can be read ({why})"),
            Error::Missing(what) => write!(f, "it has no {what}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// A table: where its b-tree starts and what its columns are called.
pub struct Table {
    root: u32,
    columns: Vec<String>,
    /// The column that is the row's own id (`INTEGER PRIMARY KEY`): the
    /// record stores nothing for it.
    alias: Option<usize>,
}

pub struct Database {
    file: File,
    page_size: usize,
    /// Page content minus the bytes the file reserves at each page's end.
    usable: usize,
    /// How many pages the file holds; a page number past it is damage.
    pages: u64,
    /// Pages from the write-ahead log, which are newer than the file's.
    wal: HashMap<u32, Rc<Vec<u8>>>,
}

const HEADER: &[u8; 16] = b"SQLite format 3\0";
/// A b-tree is never this deep; a file that says otherwise is damaged.
const MAX_DEPTH: u32 = 24;
/// The largest row this reader takes. A history row is well under a
/// kilobyte; a larger one is skipped rather than read into memory.
pub const MAX_PAYLOAD: usize = 1 << 20;
/// A scan stops after this many rows, whatever the table holds. Years of
/// heavy browsing is a few hundred thousand visits.
pub const MAX_ROWS: usize = 2_000_000;
/// The write-ahead log is read whole, so it has a ceiling of its own.
pub const MAX_WAL: u64 = 64 << 20;

fn be16(b: &[u8], at: usize) -> Option<usize> {
    let bytes = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let bytes = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// A variable-length integer: its value and how many bytes it took.
fn varint(b: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for i in 0..9 {
        let byte = *b.get(at.checked_add(i)?)?;
        if i == 8 {
            return Some(((value << 8) | byte as u64, 9));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

impl Database {
    /// Opens a database file read-only, and applies the write-ahead log
    /// beside it (`<file>-wal`) when there is one.
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = [0u8; 100];
        file.read_exact(&mut head)
            .map_err(|_| Error::Format("too short"))?;
        if &head[..16] != HEADER {
            return Err(Error::Format("not SQLite"));
        }
        let page_size = match be16(&head, 16) {
            Some(1) => 65536,
            Some(n) if (512..=32768).contains(&n) && n.is_power_of_two() => n,
            _ => return Err(Error::Format("page size")),
        };
        let usable = page_size - head[20] as usize;
        if usable < 480 {
            return Err(Error::Format("page size"));
        }
        // 0 is a file whose text encoding was never needed (no table yet).
        if !matches!(be32(&head, 56), Some(0 | 1)) {
            return Err(Error::Format("text is not UTF-8"));
        }
        let mut name = path.as_os_str().to_owned();
        name.push("-wal");
        let wal = read_capped(Path::new(&name), MAX_WAL)
            .map(|bytes| read_wal(&bytes, page_size))
            .unwrap_or_default();
        let wal_pages = wal.keys().copied().max().unwrap_or(0) as u64;
        Ok(Self {
            file,
            page_size,
            usable,
            pages: (len / page_size as u64).max(wal_pages),
            wal,
        })
    }

    fn page(&mut self, number: u32) -> Result<Rc<Vec<u8>>> {
        if number == 0 || number as u64 > self.pages {
            return Err(Error::Format("a page number outside the file"));
        }
        if let Some(page) = self.wal.get(&number) {
            return Ok(Rc::clone(page));
        }
        let mut page = vec![0u8; self.page_size];
        self.file
            .seek(SeekFrom::Start((number as u64 - 1) * self.page_size as u64))?;
        self.file
            .read_exact(&mut page)
            .map_err(|_| Error::Format("a page is missing"))?;
        Ok(Rc::new(page))
    }

    /// The table with this name.
    pub fn table(&mut self, name: &str) -> Result<Table> {
        let mut found = None;
        let master = Table {
            root: 1,
            columns: ["type", "name", "tbl_name", "rootpage", "sql"]
                .map(String::from)
                .to_vec(),
            alias: None,
        };
        self.scan(&master, &["type", "name", "rootpage", "sql"], |_, row| {
            if row[0].text() == Some("table") && row[1].text() == Some(name) {
                let root = row[2].int().and_then(|r| u32::try_from(r).ok());
                if let (Some(root), Some(sql)) = (root, row[3].text()) {
                    found = Some((root, sql.to_string()));
                    return false;
                }
            }
            true
        })?;
        let (root, sql) = found.ok_or_else(|| Error::Missing(format!("{name} table")))?;
        let (columns, alias) = columns_of(&sql);
        Ok(Table {
            root,
            columns,
            alias,
        })
    }

    /// Calls `each` with the row's id and the named columns of every row of
    /// the table, in order, until it answers false or [`MAX_ROWS`] rows have
    /// been read.
    pub fn scan(
        &mut self,
        table: &Table,
        columns: &[&str],
        mut each: impl FnMut(i64, &[Value]) -> bool,
    ) -> Result<()> {
        let want: Vec<usize> = columns
            .iter()
            .map(|c| {
                table
                    .columns
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(c))
                    .ok_or_else(|| Error::Missing(format!("{c} column")))
            })
            .collect::<Result<_>>()?;
        let mut row = Vec::with_capacity(want.len());
        let mut seen = HashSet::new();
        let mut rows = 0usize;
        self.walk(table.root, 0, &mut seen, &mut |id, payload| {
            rows += 1;
            if rows > MAX_ROWS {
                return false;
            }
            row.clear();
            if !record(payload, &want, table.alias, id, &mut row) {
                // A row that can't be read is skipped, not fatal.
                return true;
            }
            each(id, &row)
        })?;
        Ok(())
    }

    /// Visits a table b-tree's rows in order; false from `visit` stops. A
    /// payload that cannot be read (too large, cut short) is passed as
    /// empty, which no record reads as a row.
    fn walk(
        &mut self,
        number: u32,
        depth: u32,
        seen: &mut HashSet<u32>,
        visit: &mut dyn FnMut(i64, &[u8]) -> bool,
    ) -> Result<bool> {
        if depth > MAX_DEPTH {
            return Err(Error::Format("b-tree too deep"));
        }
        if !seen.insert(number) {
            return Err(Error::Format("a page is used twice"));
        }
        let page = self.page(number)?;
        let at = if number == 1 { 100 } else { 0 };
        let kind = *page.get(at).ok_or(Error::Format("short page"))?;
        let cells = be16(&page, at + 3).ok_or(Error::Format("short page"))?;
        match kind {
            // Interior: each cell leads to a child; the last child is
            // named in the header.
            0x05 => {
                let pointers = at + 12;
                for i in 0..cells {
                    let cell = be16(&page, pointers + 2 * i).ok_or(Error::Format("cell"))?;
                    let child = be32(&page, cell).ok_or(Error::Format("cell outside page"))?;
                    if !self.walk(child, depth + 1, seen, visit)? {
                        return Ok(false);
                    }
                }
                let last = be32(&page, at + 8).ok_or(Error::Format("short page"))?;
                self.walk(last, depth + 1, seen, visit)
            }
            // Leaf: the rows themselves.
            0x0d => {
                let pointers = at + 8;
                for i in 0..cells {
                    let cell = be16(&page, pointers + 2 * i).ok_or(Error::Format("cell"))?;
                    let (size, a) = varint(&page, cell).ok_or(Error::Format("cell"))?;
                    let (id, b) = varint(&page, cell + a).ok_or(Error::Format("cell"))?;
                    let payload = self
                        .payload(&page, cell + a + b, size, seen)
                        .unwrap_or_default();
                    if !visit(id as i64, &payload) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Err(Error::Format("not a table page")),
        }
    }

    /// A cell's whole payload: what is on the page, then the overflow pages
    /// it continues on. None when it is too large or damaged.
    fn payload(
        &mut self,
        page: &[u8],
        start: usize,
        size: u64,
        seen: &mut HashSet<u32>,
    ) -> Option<Vec<u8>> {
        let size = usize::try_from(size).ok().filter(|s| *s <= MAX_PAYLOAD)?;
        let u = self.usable;
        let max_local = u - 35;
        if size <= max_local {
            return page
                .get(start..start.checked_add(size)?)
                .map(<[u8]>::to_vec);
        }
        let min_local = (u - 12) * 32 / 255 - 23;
        let k = min_local + (size - min_local) % (u - 4);
        let local = if k <= max_local { k } else { min_local };
        let mut out = Vec::with_capacity(size);
        out.extend_from_slice(page.get(start..start.checked_add(local)?)?);
        let mut next = be32(page, start + local)?;
        while out.len() < size {
            // An overflow chain that comes back to a page is damage.
            if next == 0 || !seen.insert(next) {
                return None;
            }
            let overflow = self.page(next).ok()?;
            let take = (size - out.len()).min(u - 4);
            out.extend_from_slice(overflow.get(4..4 + take)?);
            next = be32(&overflow, 0)?;
        }
        Some(out)
    }
}

/// A file's bytes, refused when it is larger than `cap`.
fn read_capped(path: &Path, cap: u64) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Err(std::io::Error::other("too large"));
    }
    Ok(bytes)
}

/// The wanted columns of one record, appended to `out`. False when the
/// record is damaged.
fn record(
    payload: &[u8],
    want: &[usize],
    alias: Option<usize>,
    id: i64,
    out: &mut Vec<Value>,
) -> bool {
    let Some((header, mut at)) = varint(payload, 0) else {
        return false;
    };
    let Some(header) = usize::try_from(header).ok().filter(|h| *h <= payload.len()) else {
        return false;
    };
    // Serial types, and where each column's bytes start.
    let mut types = Vec::new();
    while at < header {
        let Some((t, n)) = varint(payload, at) else {
            return false;
        };
        types.push(t);
        at += n;
    }
    let mut offsets = Vec::with_capacity(types.len());
    let mut body = header;
    for t in &types {
        offsets.push(body);
        let len = match t {
            0 | 8 | 9 => 0,
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 6,
            6 | 7 => 8,
            t if *t >= 12 => match usize::try_from((*t - 12) / 2) {
                Ok(n) => n,
                Err(_) => return false,
            },
            _ => return false,
        };
        body = match body.checked_add(len) {
            Some(b) => b,
            None => return false,
        };
    }
    for &column in want {
        if alias == Some(column) {
            out.push(Value::Int(id));
            continue;
        }
        let (Some(&t), Some(&from)) = (types.get(column), offsets.get(column)) else {
            // A column added after the row was written reads as null.
            out.push(Value::Null);
            continue;
        };
        let value = match t {
            0 => Value::Null,
            8 => Value::Int(0),
            9 => Value::Int(1),
            1..=6 => {
                let len = [1, 2, 3, 4, 6, 8][t as usize - 1];
                let Some(bytes) = from.checked_add(len).and_then(|end| payload.get(from..end))
                else {
                    return false;
                };
                let mut n = if bytes[0] & 0x80 != 0 { -1i64 } else { 0 };
                for byte in bytes {
                    n = (n << 8) | *byte as i64;
                }
                Value::Int(n)
            }
            7 => {
                let Some(bytes) = from.checked_add(8).and_then(|end| payload.get(from..end)) else {
                    return false;
                };
                let mut eight = [0u8; 8];
                eight.copy_from_slice(bytes);
                Value::Real(f64::from_be_bytes(eight))
            }
            t if t >= 12 => {
                let Ok(len) = usize::try_from((t - 12) / 2) else {
                    return false;
                };
                let Some(bytes) = from.checked_add(len).and_then(|end| payload.get(from..end))
                else {
                    return false;
                };
                if t % 2 == 0 {
                    Value::Blob(bytes.to_vec())
                } else {
                    Value::Text(String::from_utf8_lossy(bytes).into_owned())
                }
            }
            _ => return false,
        };
        out.push(value);
    }
    true
}

/// A table's column names from its CREATE statement, and which one is the
/// row id.
fn columns_of(sql: &str) -> (Vec<String>, Option<usize>) {
    let Some(open) = sql.find('(') else {
        return (Vec::new(), None);
    };
    let close = sql.rfind(')').filter(|c| *c > open).unwrap_or(sql.len());
    let body = &sql[open + 1..close];
    // Split at commas outside parentheses and quotes.
    let mut parts = Vec::new();
    let (mut depth, mut quote, mut from) = (0i32, None::<char>, 0usize);
    for (i, c) in body.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"' | '`') => quote = Some(c),
            (None, '[') => quote = Some(']'),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(&body[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[from..]);
    let mut columns = Vec::new();
    let mut alias = None;
    for part in parts {
        let part = part.trim();
        let first = column_name(part);
        if matches!(
            first.to_ascii_uppercase().as_str(),
            "PRIMARY" | "UNIQUE" | "CHECK" | "FOREIGN" | "CONSTRAINT"
        ) {
            continue;
        }
        // `INTEGER PRIMARY KEY DESC` is an ordinary column in SQLite, the one
        // quirk of the rule.
        let upper = part.to_ascii_uppercase();
        if upper.contains("INTEGER PRIMARY KEY") && !upper.contains("PRIMARY KEY DESC") {
            alias = Some(columns.len());
        }
        columns.push(first.to_string());
    }
    (columns, alias)
}

/// The name a column definition starts with: a quoted name whole (it may
/// hold spaces), or a bare word up to a space or a parenthesis.
fn column_name(part: &str) -> &str {
    let close = match part.chars().next() {
        Some('"') => '"',
        Some('`') => '`',
        Some('[') => ']',
        Some('\'') => '\'',
        _ => {
            let end = part
                .find(|c: char| c.is_whitespace() || c == '(')
                .unwrap_or(part.len());
            return &part[..end];
        }
    };
    let rest = &part[1..];
    &rest[..rest.find(close).unwrap_or(rest.len())]
}

/// The pages a write-ahead log holds as of its last commit. Frames carry
/// the salt of the log's current run and a running checksum; the first
/// frame that fails either, and everything after it, is a write that never
/// finished.
fn read_wal(bytes: &[u8], page_size: usize) -> HashMap<u32, Rc<Vec<u8>>> {
    let mut pages = HashMap::new();
    let (Some(magic), Some(size)) = (be32(bytes, 0), be32(bytes, 8)) else {
        return pages;
    };
    if bytes.len() < 32 || (magic != 0x377f_0682 && magic != 0x377f_0683) {
        return pages;
    }
    if size as usize != page_size {
        return pages;
    }
    let big = magic & 1 == 1;
    // Every index below is inside `bytes`: the header is 32 bytes, and a
    // frame is only read while a whole one remains.
    let word = |at: usize| -> u32 {
        let w = [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]];
        if big {
            u32::from_be_bytes(w)
        } else {
            u32::from_le_bytes(w)
        }
    };
    let sum = |mut s: (u32, u32), from: usize, to: usize| -> (u32, u32) {
        let mut at = from;
        while at + 8 <= to {
            s.0 = s.0.wrapping_add(word(at)).wrapping_add(s.1);
            s.1 = s.1.wrapping_add(word(at + 4)).wrapping_add(s.0);
            at += 8;
        }
        s
    };
    let be = |at: usize| be32(bytes, at).unwrap_or(0);
    let mut running = sum((0, 0), 0, 24);
    if running != (be(24), be(28)) {
        return pages;
    }
    let salt = (be(16), be(20));
    let frame = 24 + page_size;
    let mut staged: Vec<(u32, usize)> = Vec::new();
    let mut at = 32;
    while at + frame <= bytes.len() {
        if (be(at + 8), be(at + 12)) != salt {
            break;
        }
        running = sum(running, at, at + 8);
        running = sum(running, at + 24, at + frame);
        if running != (be(at + 16), be(at + 20)) {
            break;
        }
        staged.push((be(at), at + 24));
        // A commit frame names the database's size after it: everything
        // staged so far is real.
        if be(at + 4) != 0 {
            for (number, from) in staged.drain(..) {
                pages.insert(number, Rc::new(bytes[from..from + page_size].to_vec()));
            }
        }
        at += frame;
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(path: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/browser-import")
            .join(path)
    }

    #[test]
    fn varints_run_one_to_nine_bytes() {
        assert_eq!(varint(&[0x7f], 0), Some((127, 1)));
        assert_eq!(varint(&[0x81, 0x00], 0), Some((128, 2)));
        assert_eq!(varint(&[0xff; 9], 0), Some((u64::MAX, 9)));
        assert_eq!(varint(&[0x81], 0), None);
        assert_eq!(varint(&[0x01], usize::MAX), None);
    }

    #[test]
    fn column_names_come_from_the_create_statement() {
        let (columns, alias) = columns_of(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY AUTOINCREMENT,url LONGVARCHAR,title LONGVARCHAR,visit_count INTEGER DEFAULT 0 NOT NULL, \"last visit\" INTEGER, PRIMARY KEY(a, b), CHECK(x IN (1,2)))",
        );
        assert_eq!(
            columns,
            ["id", "url", "title", "visit_count", "last visit"].map(String::from)
        );
        assert_eq!(alias, Some(0));
        let (columns, alias) = columns_of("CREATE TABLE t (a TEXT, b INTEGER PRIMARY KEY DESC)");
        assert_eq!(columns, ["a", "b"].map(String::from));
        // A descending key is not a row-id alias in SQLite.
        assert_eq!(alias, None);
        // Damaged statements give what they can, never a panic.
        for sql in ["", "(", ")(", "CREATE TABLE t (", "CREATE TABLE t )x("] {
            let _ = columns_of(sql);
        }
    }

    #[test]
    fn a_table_spread_over_interior_and_overflow_pages_reads_whole() {
        let mut db = Database::open(&fixture("chromium/Default/History")).unwrap();
        let visits = db.table("visits").unwrap();
        let mut ids = Vec::new();
        db.scan(&visits, &["id", "url"], |id, row| {
            assert_eq!(row[0].int(), Some(id));
            ids.push(id);
            true
        })
        .unwrap();
        // 240 ordinary visits, one old one and two in frames (make.py).
        assert_eq!(ids.len(), 243);
        assert!(ids.windows(2).all(|w| w[0] < w[1]));
        let urls = db.table("urls").unwrap();
        let mut long = None;
        db.scan(&urls, &["url"], |_, row| {
            let url = row[0].text().unwrap_or("");
            if url.starts_with("https://long.example.com/") {
                long = Some(url.len());
            }
            true
        })
        .unwrap();
        // 25 + 1500: the row continues on an overflow page.
        assert_eq!(long, Some(1525));
    }

    #[test]
    fn the_write_ahead_log_is_applied() {
        let mut db = Database::open(&fixture(
            "firefox/Profiles/abcd1234.default-release/places.sqlite",
        ))
        .unwrap();
        let marks = db.table("moz_bookmarks").unwrap();
        let mut guids = Vec::new();
        db.scan(&marks, &["guid"], |_, row| {
            guids.push(row[0].text().unwrap_or("").to_string());
            true
        })
        .unwrap();
        // Written after the last checkpoint: only the log has it.
        assert!(guids.iter().any(|g| g == "bmmobile0001"));
    }

    #[test]
    fn a_missing_table_or_column_is_said() {
        let mut db = Database::open(&fixture("safari/History.db")).unwrap();
        assert!(matches!(db.table("nope"), Err(Error::Missing(_))));
        let items = db.table("history_items").unwrap();
        assert!(matches!(
            db.scan(&items, &["nope"], |_, _| true),
            Err(Error::Missing(_))
        ));
    }

    #[test]
    fn what_is_not_sqlite_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x");
        for bytes in [&b""[..], b"SQLite format 3", &[0u8; 200][..]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(Database::open(&path).is_err());
        }
    }

    /// Every fixture database, damaged at random a few hundred times: the
    /// reader answers an error or some rows, and never panics or hangs.
    #[test]
    fn a_damaged_file_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for (name, table) in [
            ("chromium/Default/History", "visits"),
            ("chromium/Default/History", "urls"),
            (
                "firefox/Profiles/abcd1234.default-release/places.sqlite",
                "moz_bookmarks",
            ),
            ("safari/History.db", "history_visits"),
        ] {
            let clean = std::fs::read(fixture(name)).unwrap();
            for round in 0..150 {
                let mut bytes = clean.clone();
                let flips = 1 + (next() % 24) as usize;
                for _ in 0..flips {
                    // Past the 16-byte magic, so most rounds get to the tree.
                    let at = 16 + (next() as usize) % (bytes.len() - 16);
                    bytes[at] = next() as u8;
                }
                if round % 10 == 0 {
                    bytes.truncate((next() as usize) % bytes.len());
                }
                let path = dir.path().join("damaged");
                std::fs::write(&path, &bytes).unwrap();
                if let Ok(mut db) = Database::open(&path) {
                    if let Ok(t) = db.table(table) {
                        let columns: Vec<&str> = t.columns.iter().map(String::as_str).collect();
                        let _ = db.scan(&t, &columns, |_, _| true);
                    }
                }
            }
        }
    }

    #[test]
    fn a_tree_that_points_back_at_itself_ends() {
        // A minimal file whose page 1 is an interior page naming itself.
        let mut bytes = vec![0u8; 1024];
        bytes[..16].copy_from_slice(HEADER);
        bytes[16..18].copy_from_slice(&1024u16.to_be_bytes());
        bytes[100] = 0x05;
        bytes[103..105].copy_from_slice(&0u16.to_be_bytes());
        bytes[108..112].copy_from_slice(&1u32.to_be_bytes());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("loop");
        std::fs::write(&path, &bytes).unwrap();
        let mut db = Database::open(&path).unwrap();
        assert!(matches!(db.table("x"), Err(Error::Format(_))));
    }
}
