//! Bounded line reading for a harness's child streams.

use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// The hard cap on one line from a harness: 32 MiB of content, the
/// delimiter excluded. A frame can carry pictures - an agent's
/// screenshot rides its tool result as base64, and claude repeats the
/// result beside the message - so a full-page capture alone can pass
/// 4 MiB; a frame past this one is a runaway (a delta flood, a bug), not
/// one worth parsing.
pub const MAX_LINE_BYTES: usize = 32 * 1024 * 1024;

/// Reads lines from a child stream with a hard cap per line (harness
/// audit F11, the bounded-read half; the idle-watchdog half is
/// deliberately not here - no timers).
///
/// `read_line` into a String has two ways to hurt a live session: the
/// buffer grows until a newline arrives, so a harness emitting one giant
/// unterminated line grows daemon memory without bound; and a String
/// target turns invalid UTF-8 into `Err(InvalidData)`, which the drivers
/// treated as a fatal break while the process was still alive. This
/// reader buffers at most [`MAX_LINE_BYTES`] of a line at a time: an
/// over-long line is dropped whole (with a note on the daemon's stderr,
/// where the drivers already note anomalies) and reading continues at
/// the next line, and the bytes decode lossily (`from_utf8_lossy`
/// semantics), so a corrupt byte costs the line it sits in, never the
/// session.
pub struct LineReader<R> {
    inner: R,
    cap: usize,
    /// The current line's bytes; only grown while the line still fits the
    /// cap, so memory stays bounded no matter what the child emits.
    buf: Vec<u8>,
}

impl<R: AsyncBufRead + Unpin> LineReader<R> {
    /// Wraps a buffered child stream (the drivers pass
    /// `BufReader<ChildStdout>` / `BufReader<ChildStderr>`) with the
    /// default cap.
    pub fn new(inner: R) -> Self {
        Self::with_cap(inner, MAX_LINE_BYTES)
    }

    /// Same, with an explicit cap - the unit tests exercise the boundary
    /// with a tiny one.
    pub fn with_cap(inner: R, cap: usize) -> Self {
        Self {
            inner,
            cap,
            buf: Vec::new(),
        }
    }

    /// Reads the next line. `\n` and `\r\n` are both accepted (the ICRNL
    /// lesson) and the delimiter is trimmed, so the returned String is
    /// exactly what the drivers used to get from `read_line` plus
    /// `trim_end_matches(['\r', '\n'])`. Returns `Ok(None)` at end of
    /// stream. An over-long line never surfaces: it is dropped (with a
    /// note on the daemon's stderr) and reading continues.
    pub async fn next_line(&mut self) -> io::Result<Option<String>> {
        loop {
            let chunk = match self.inner.fill_buf().await {
                Ok(chunk) => chunk,
                Err(e) => return Err(e),
            };
            if chunk.is_empty() {
                // End of stream; a final unterminated line is still a line
                // (read_line semantics).
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let bytes = std::mem::take(&mut self.buf);
                return Ok(Some(decode(bytes)));
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(i) => {
                    let line_len = self.buf.len() + i;
                    if line_len > self.cap {
                        // The newline ends an over-long line: drop it
                        // whole, and never hand its bytes to a parser.
                        self.buf.clear();
                        self.inner.consume(i + 1);
                        note_overlong(self.cap, line_len as u64 + 1);
                        continue;
                    }
                    self.buf.extend_from_slice(&chunk[..i]);
                    self.inner.consume(i + 1);
                    let bytes = std::mem::take(&mut self.buf);
                    return Ok(Some(decode(bytes)));
                }
                None => {
                    let n = chunk.len();
                    if self.buf.len() + n > self.cap {
                        // Past the cap with no newline in sight: the line
                        // is over-long whatever comes next. Discard in
                        // transport-sized chunks to the newline (or end
                        // of stream) - the discard never lands in memory.
                        let total = (self.buf.len() + n) as u64;
                        self.buf.clear();
                        self.inner.consume(n);
                        self.skip_to_newline(total).await?;
                        continue;
                    }
                    self.buf.extend_from_slice(chunk);
                    self.inner.consume(n);
                }
            }
        }
    }

    /// Discards the rest of an over-long line, counting its bytes for the
    /// note. Returns at the newline (the next line starts clean) or at
    /// end of stream.
    async fn skip_to_newline(&mut self, mut total: u64) -> io::Result<()> {
        loop {
            let chunk = self.inner.fill_buf().await?;
            if chunk.is_empty() {
                note_overlong(self.cap, total);
                return Ok(());
            }
            match chunk.iter().position(|&b| b == b'\n') {
                Some(j) => {
                    self.inner.consume(j + 1);
                    note_overlong(self.cap, total + j as u64 + 1);
                    return Ok(());
                }
                None => {
                    let n = chunk.len();
                    total += n as u64;
                    self.inner.consume(n);
                }
            }
        }
    }
}

/// One line's bytes to its driver string: trailing `\r`/`\n` trimmed,
/// then decoded lossily so invalid UTF-8 becomes U+FFFD in place - a
/// clean line stays byte-identical, a corrupt one fails the frame parser,
/// and neither kills the reader.
fn decode(bytes: Vec<u8>) -> String {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The over-length note goes to the daemon's own stderr - the same place
/// the drivers already note anomalies (codex's turn errors) - not onto
/// the session's event wire.
fn note_overlong(cap: usize, total: u64) {
    eprintln!(
        "harness stream: dropped a {total}-byte line over the {cap}-byte cap instead of buffering it (harness audit F11)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reader over static bytes, capped small so the boundary cases
    /// are cheap to build.
    fn reader(
        bytes: &'static [u8],
        cap: usize,
    ) -> LineReader<tokio::io::BufReader<std::io::Cursor<&'static [u8]>>> {
        LineReader::with_cap(tokio::io::BufReader::new(std::io::Cursor::new(bytes)), cap)
    }

    #[tokio::test]
    async fn normal_lines_round_trip_byte_identical() {
        let bytes = b"one\ntwo\r\nthree \n\nlast";
        let mut lines = reader(bytes, MAX_LINE_BYTES);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("one"));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("two"));
        // Trailing spaces are content: only the delimiters are trimmed.
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("three "));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some(""));
        // The final unterminated line still reads (read_line semantics).
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("last"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn invalid_utf8_is_a_lossy_line_not_a_dead_reader() {
        // \xe9 followed by 'm' is not a valid sequence: the byte becomes
        // U+FFFD and the rest of the line survives around it.
        let bytes = b"turn \xe9mid\nnext\n";
        let mut lines = reader(bytes, MAX_LINE_BYTES);
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("turn \u{FFFD}mid")
        );
        // The session lives: the lines after the corrupt one read intact.
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("next"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_oversized_line_is_dropped_and_reading_continues() {
        // 16 content bytes over an 8-byte cap, newline in the same
        // transport chunk: dropped whole, never surfaced to the parser.
        let bytes = b"ok\n0123456789abcdef\ntail\n";
        let mut lines = reader(bytes, 8);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("ok"));
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("tail"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_oversized_line_spreading_past_the_transport_chunk_is_dropped_too() {
        // 100 KiB with no newline: the cap trips long before the newline
        // is ever seen, so the discard runs in transport-sized rounds.
        let mut bytes = vec![b'x'; 100 * 1024];
        bytes.extend_from_slice(b"\nafter\n");
        let mut lines =
            LineReader::with_cap(tokio::io::BufReader::new(std::io::Cursor::new(bytes)), 8);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("after"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn an_unterminated_oversized_line_ends_cleanly_at_eof() {
        // 9 bytes over an 8-byte cap and no newline ever: dropped, and
        // the reader still reports end of stream rather than hanging or
        // surfacing a partial monster.
        let bytes = b"0123456789";
        let mut lines = reader(bytes, 8);
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_line_of_exactly_the_cap_is_kept() {
        // 8 content bytes at an 8-byte cap: kept; one byte more: dropped.
        let bytes = b"12345678\n123456789\ntail\n";
        let mut lines = reader(bytes, 8);
        assert_eq!(
            lines.next_line().await.unwrap().as_deref(),
            Some("12345678")
        );
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("tail"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_crlf_line_at_the_cap_is_kept() {
        // The \r is content for the cap (the newline is the delimiter),
        // then trimmed from the delivered line.
        let bytes = b"1234567\r\n";
        let mut lines = reader(bytes, 8);
        assert_eq!(lines.next_line().await.unwrap().as_deref(), Some("1234567"));
        assert_eq!(lines.next_line().await.unwrap(), None);
    }
}
