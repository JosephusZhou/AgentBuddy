//! On-disk log file for route aggregation entries.
//!
//! Format: JSON Lines (one serialized `LogEntry` per line, `\n` terminated).
//! This is the same shape used by most observability stacks (Vector, Loki,
//! Loki Promtail, etc.) and is trivial to `tail -f`, `grep`, or pipe into
//! `jq` for field extraction. The on-disk file is the source of truth for
//! "what happened recently" — the in-memory `LogStore` is just a fast UI
//! window. When the diagnostics screenshot isn't enough, the user can read
//! the file directly.
//!
//! Failures to open or write the file are swallowed: the in-memory log stays
//! usable, and we don't want a permission glitch to take down the proxy.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::log::LogEntry;

/// One serialized line on disk. Held inside a `Mutex` so concurrent pushers
/// don't interleave their writes.
struct Inner {
    writer: BufWriter<File>,
    path: PathBuf,
}

/// Keep the durable diagnostic log bounded. The in-memory ring is bounded
/// separately, but an append-only file would otherwise grow without limit.
const MAX_LOG_FILE_BYTES: u64 = 16 * 1024 * 1024;

fn rotated_path(path: &Path) -> PathBuf {
    path.with_extension("log.1")
}

/// Cheap, thread-safe handle to the on-disk log file.
#[derive(Clone)]
pub struct LogFile {
    inner: Arc<Mutex<Inner>>,
}

impl LogFile {
    /// Open the file in append mode, creating the parent directory if
    /// needed. Returns `None` if the file cannot be opened (e.g. permission
    /// denied) — callers should treat logging as advisory and not fail the
    /// surrounding operation.
    pub fn open(path: &Path) -> Option<Self> {
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                eprintln!(
                    "[route-aggregation] failed to create log dir {}: {}",
                    dir.display(),
                    e
                );
                return None;
            }
        }
        let file = match OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!(
                    "[route-aggregation] failed to open log file {}: {}",
                    path.display(),
                    e
                );
                return None;
            }
        };
        Some(Self {
            inner: Arc::new(Mutex::new(Inner {
                writer: BufWriter::new(file),
                path: path.to_path_buf(),
            })),
        })
    }

    /// Returns the path the log file was opened at, for surfacing in the UI.
    pub fn path(&self) -> PathBuf {
        self.inner.lock().unwrap().path.clone()
    }

    /// Serialize `entry` and append it as a single JSON line. Failures are
    /// logged to stderr but never propagate — the in-memory ring is the
    /// primary UI surface; the file is best-effort.
    pub fn write_entry(&self, entry: &LogEntry) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let line = match serde_json::to_string(entry) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[route-aggregation] serialize log entry failed: {}", e);
                return;
            }
        };
        let incoming = line.len() as u64 + 1;
        if let Ok(size) = inner.writer.get_ref().metadata().map(|m| m.len()) {
            if size.saturating_add(incoming) > MAX_LOG_FILE_BYTES {
                let rotated = rotated_path(&inner.path);
                let _ = inner.writer.flush();
                let _ = std::fs::remove_file(&rotated);
                if std::fs::rename(&inner.path, &rotated).is_ok() {
                    match OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&inner.path)
                    {
                        Ok(file) => inner.writer = BufWriter::new(file),
                        Err(e) => eprintln!("[route-aggregation] reopen rotated log failed: {}", e),
                    }
                } else if let Err(e) = inner.writer.get_ref().set_len(0) {
                    // Windows may not allow renaming an open file. Truncating
                    // the active file still enforces the size bound there.
                    eprintln!("[route-aggregation] rotate and truncate log failed: {}", e);
                }
            }
        }
        if let Err(e) = inner.writer.write_all(line.as_bytes()) {
            eprintln!("[route-aggregation] write log line failed: {}", e);
            return;
        }
        if let Err(e) = inner.writer.write_all(b"\n") {
            eprintln!("[route-aggregation] write log newline failed: {}", e);
            return;
        }
        // Flush on every line is overkill but the file is small, the volume
        // is bounded by the 2000-entry ring, and the user wants `tail -f` to
        // see entries immediately. The cost is a syscall per request.
        if let Err(e) = inner.writer.flush() {
            eprintln!("[route-aggregation] flush log failed: {}", e);
        }
    }

    /// Clear the active log after the in-memory view is cleared.
    pub fn clear(&self) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Err(e) = inner.writer.flush() {
            eprintln!("[route-aggregation] flush before clear failed: {}", e);
            return;
        }
        if let Err(e) = inner.writer.get_ref().set_len(0) {
            eprintln!("[route-aggregation] clear log failed: {}", e);
        }
        let rotated = rotated_path(&inner.path);
        if let Err(e) = std::fs::remove_file(&rotated) {
            if e.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "[route-aggregation] clear rotated log {} failed: {}",
                    rotated.display(),
                    e
                );
            }
        }
    }
}
