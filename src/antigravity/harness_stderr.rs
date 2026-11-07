//! The harness's stderr: drained from the moment it starts, so a full pipe
//! can never block it, and kept as a bounded tail for error messages.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::io::{AsyncBufReadExt, BufReader};

use super::session::WireContext;
use crate::wire::WireEvent;

/// Maximum stderr lines retained for diagnostics.
const STDERR_RING_CAPACITY: usize = 200;

/// Maximum bytes of stderr retained for diagnostics, across the lines the
/// ring holds. The oldest lines go first.
const STDERR_RING_MAX_BYTES: usize = 64 * 1024;

/// Maximum bytes kept of one stderr line. The rest, up to the next
/// newline, is read and dropped: a binary that writes without newlines
/// (the wrong binary, or a crash dump) must not grow the line buffer
/// without bound.
const MAX_STDERR_LINE_BYTES: usize = 8 * 1024;

/// The tail of the harness's stderr, kept for diagnostics: at most
/// [`STDERR_RING_CAPACITY`] lines and [`STDERR_RING_MAX_BYTES`] bytes, the
/// oldest dropped first. The latest line is always kept.
#[derive(Debug, Default)]
pub(super) struct StderrRing {
    lines: VecDeque<String>,
    bytes: usize,
}

impl StderrRing {
    fn push(&mut self, line: String) {
        self.bytes += line.len();
        self.lines.push_back(line);
        while self.lines.len() > STDERR_RING_CAPACITY
            || (self.bytes > STDERR_RING_MAX_BYTES && self.lines.len() > 1)
        {
            if let Some(oldest) = self.lines.pop_front() {
                self.bytes -= oldest.len();
            }
        }
    }

    /// The retained lines, joined by newlines.
    pub(super) fn tail(&self) -> String {
        self.lines.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

/// Locks the ring. A panic while it was held leaves the mutex poisoned, but
/// the lines in it are still whole, so the lock is taken anyway: the drain
/// keeps recording, and an error message still gets its stderr tail,
/// rather than every later line and error panicking in turn.
pub(super) fn lock_ring(ring: &Mutex<StderrRing>) -> MutexGuard<'_, StderrRing> {
    ring.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How a stderr line read by [`drain_stderr`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnd {
    /// At a newline (stripped, with a `\r` before it).
    Newline,
    /// At EOF, with no newline.
    Eof,
    /// At [`MAX_STDERR_LINE_BYTES`]; the rest of the line is dropped.
    Truncated,
}

/// Drains a stderr stream line by line into the diagnostic ring,
/// forwarding each line to `tracing` and the wire-inspection layer.
///
/// Reads bytes rather than UTF-8 lines: this sits at the wrong-binary
/// trust boundary, and a single non-UTF-8 line must not stop the drain —
/// a stopped drain lets the OS pipe buffer fill and deadlocks the child.
/// Invalid UTF-8 is replaced lossily; only EOF or a genuine I/O error ends
/// the loop. For the same reason a line is capped at
/// [`MAX_STDERR_LINE_BYTES`]: its head is recorded as soon as the cap is
/// reached, marked as cut, and the rest up to the next newline is read
/// and dropped, so output with no newlines holds a bounded buffer.
pub(super) async fn drain_stderr<R>(stderr: R, ring: Arc<Mutex<StderrRing>>, wire: WireContext)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut reader = BufReader::new(stderr);
    let mut line = Vec::new();
    // Past the cap of the current line: drop bytes until its newline.
    let mut skipping = false;
    loop {
        let chunk = match reader.fill_buf().await {
            Ok(chunk) => chunk,
            Err(e) => {
                tracing::debug!("harness stderr drain ended on read error: {e}");
                return;
            }
        };
        if chunk.is_empty() {
            // EOF: the child closed its stderr. A last line without a
            // newline still counts.
            if !line.is_empty() {
                record_stderr_line(&line, LineEnd::Eof, &ring, &wire);
            }
            return;
        }
        let newline = chunk.iter().position(|&byte| byte == b'\n');
        let end = newline.unwrap_or(chunk.len());
        if skipping {
            skipping = newline.is_none();
        } else {
            let room = MAX_STDERR_LINE_BYTES - line.len();
            line.extend_from_slice(&chunk[..end.min(room)]);
            if end > room {
                record_stderr_line(&line, LineEnd::Truncated, &ring, &wire);
                line.clear();
                skipping = newline.is_none();
            } else if newline.is_some() {
                record_stderr_line(&line, LineEnd::Newline, &ring, &wire);
                line.clear();
            }
        }
        let consumed = newline.map_or(chunk.len(), |at| at + 1);
        reader.consume(consumed);
    }
}

/// Records one stderr line: `tracing`, the wire-inspection layer and the
/// ring.
fn record_stderr_line(bytes: &[u8], end: LineEnd, ring: &Mutex<StderrRing>, wire: &WireContext) {
    // A newline's `\r` goes with it, as `AsyncBufReadExt::lines` strips it.
    let bytes = match end {
        LineEnd::Newline => bytes.strip_suffix(b"\r").unwrap_or(bytes),
        LineEnd::Eof | LineEnd::Truncated => bytes,
    };
    let mut line = String::from_utf8_lossy(bytes).into_owned();
    if end == LineEnd::Truncated {
        line.push_str(&format!(
            " [line cut at {MAX_STDERR_LINE_BYTES} bytes; the rest was dropped]"
        ));
    }
    tracing::debug!("harness stderr: {line}");
    wire.emit(|| WireEvent::HarnessStderr {
        id: wire.id(),
        line: line.clone(),
    });
    lock_ring(ring).push(line);
}

#[cfg(test)]
#[path = "harness_stderr_tests.rs"]
mod tests;
