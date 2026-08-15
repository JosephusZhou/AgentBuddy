//! On-disk log file for route aggregation entries.
//!
//! Format: JSON Lines (one serialized `LogEntry` per line, `\n` terminated).
//! The active file is changed at local midnight and named
//! `route_aggregation_YYYY-MM-DD.log`. Files older than seven calendar days
//! are removed by the startup cleanup and by a background periodic cleanup.
//!
//! Failures to open or write the file are swallowed: the in-memory log stays
//! usable, and we don't want a permission glitch to take down the proxy.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use chrono::{Local, NaiveDate};

use super::log::LogEntry;

/// One serialized line on disk. Held inside a `Mutex` so concurrent pushers
/// don't interleave their writes.
struct Inner {
    writer: BufWriter<File>,
    base_path: PathBuf,
    log_dir: PathBuf,
    path: PathBuf,
}

/// Keep only the current day and the six preceding calendar days.
const LOG_RETENTION_DAYS: i64 = 7;

/// Recheck retention even when there is no incoming proxy traffic.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 60);

const LOG_FILE_EXTENSION: &str = "log";

fn log_file_parts(base_path: &Path) -> (String, String) {
    let stem = base_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("route_aggregation")
        .to_string();
    let extension = base_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or(LOG_FILE_EXTENSION)
        .to_string();
    (stem, extension)
}

fn daily_path(base_path: &Path, date: NaiveDate) -> PathBuf {
    let (stem, extension) = log_file_parts(base_path);
    base_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("{stem}_{}.{extension}", date.format("%Y-%m-%d")))
}

fn log_file_prefix(base_path: &Path) -> String {
    let (stem, _) = log_file_parts(base_path);
    format!("{stem}_")
}

fn log_file_date(name: &str, prefix: &str, extension: &str) -> Option<NaiveDate> {
    let suffix = format!(".{extension}");
    let date = name.strip_prefix(prefix)?.strip_suffix(&suffix)?;
    if date.len() != 10 {
        return None;
    }
    NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()
}

fn cleanup_old_files(dir: &Path, base_path: &Path, today: NaiveDate) {
    let (_, extension) = log_file_parts(base_path);
    let prefix = log_file_prefix(base_path);
    let cutoff = today - chrono::Duration::days(LOG_RETENTION_DAYS - 1);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "[route-aggregation] failed to read log dir {}: {}",
                dir.display(),
                error
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(date) = log_file_date(&name, &prefix, &extension) else {
            continue;
        };
        if date < cutoff {
            if let Err(error) = std::fs::remove_file(&path) {
                eprintln!(
                    "[route-aggregation] failed to remove expired log {}: {}",
                    path.display(),
                    error
                );
            }
        }
    }
}

fn clear_other_files(dir: &Path, base_path: &Path, active_path: &Path) {
    let (_, extension) = log_file_parts(base_path);
    let prefix = log_file_prefix(base_path);
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "[route-aggregation] failed to read log dir {} before clear: {}",
                dir.display(),
                error
            );
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if log_file_date(name, &prefix, &extension).is_none() || path == active_path {
            continue;
        }
        if let Err(error) = std::fs::remove_file(&path) {
            eprintln!(
                "[route-aggregation] failed to clear log {}: {}",
                path.display(),
                error
            );
        }
    }
}

fn start_cleanup_worker(inner: &Arc<Mutex<Inner>>) {
    let weak: Weak<Mutex<Inner>> = Arc::downgrade(inner);
    let _ = std::thread::Builder::new()
        .name("route-aggregation-log-cleanup".to_string())
        .spawn(move || loop {
            std::thread::sleep(CLEANUP_INTERVAL);
            let Some(inner) = weak.upgrade() else {
                break;
            };
            let guard = match inner.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            cleanup_old_files(&guard.log_dir, &guard.base_path, Local::now().date_naive());
        });
}

/// Cheap, thread-safe handle to the current daily on-disk log file.
#[derive(Clone)]
pub struct LogFile {
    inner: Arc<Mutex<Inner>>,
}

impl LogFile {
    /// Open the daily log derived from `path`, creating its parent directory
    /// if needed. The supplied path is a base path; for example,
    /// `route_aggregation.log` becomes `route_aggregation_2026-08-15.log`.
    pub fn open(path: &Path) -> Option<Self> {
        if let Some(dir) = path.parent() {
            if let Err(error) = std::fs::create_dir_all(dir) {
                eprintln!(
                    "[route-aggregation] failed to create log dir {}: {}",
                    dir.display(),
                    error
                );
                return None;
            }
        }

        let log_dir = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let today = Local::now().date_naive();
        cleanup_old_files(&log_dir, path, today);
        let active_path = daily_path(path, today);
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&active_path)
        {
            Ok(file) => file,
            Err(error) => {
                eprintln!(
                    "[route-aggregation] failed to open log file {}: {}",
                    active_path.display(),
                    error
                );
                return None;
            }
        };

        let inner = Arc::new(Mutex::new(Inner {
            writer: BufWriter::new(file),
            base_path: path.to_path_buf(),
            log_dir,
            path: active_path,
        }));
        start_cleanup_worker(&inner);
        Some(Self { inner })
    }

    /// Returns the active date's path for surfacing in the UI.
    pub fn path(&self) -> PathBuf {
        self.inner.lock().unwrap().path.clone()
    }

    /// Serialize `entry` and append it as a single JSON line. Failures are
    /// logged to stderr but never propagate — the in-memory ring is the
    /// primary UI surface; the file is best-effort.
    pub fn write_entry(&self, entry: &LogEntry) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let line = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(error) => {
                eprintln!("[route-aggregation] serialize log entry failed: {}", error);
                return;
            }
        };

        let today_path = daily_path(&inner.base_path, Local::now().date_naive());
        if today_path != inner.path {
            if let Err(error) = inner.writer.flush() {
                eprintln!(
                    "[route-aggregation] flush before daily rotation failed: {}",
                    error
                );
            }
            match OpenOptions::new()
                .create(true)
                .append(true)
                .open(&today_path)
            {
                Ok(file) => {
                    inner.writer = BufWriter::new(file);
                    inner.path = today_path;
                    cleanup_old_files(&inner.log_dir, &inner.base_path, Local::now().date_naive());
                }
                Err(error) => eprintln!(
                    "[route-aggregation] reopen daily log {} failed: {}",
                    today_path.display(),
                    error
                ),
            }
        }

        if let Err(error) = inner.writer.write_all(line.as_bytes()) {
            eprintln!("[route-aggregation] write log line failed: {}", error);
            return;
        }
        if let Err(error) = inner.writer.write_all(b"\n") {
            eprintln!("[route-aggregation] write log newline failed: {}", error);
            return;
        }
        // Flush on every line so `tail -f` sees entries immediately.
        if let Err(error) = inner.writer.flush() {
            eprintln!("[route-aggregation] flush log failed: {}", error);
        }
    }

    /// Clear the active log after the in-memory view is cleared, and remove
    /// the other daily files created by this logger.
    pub fn clear(&self) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Err(error) = inner.writer.flush() {
            eprintln!("[route-aggregation] flush before clear failed: {}", error);
            return;
        }
        if let Err(error) = inner.writer.get_ref().set_len(0) {
            eprintln!("[route-aggregation] clear log failed: {}", error);
            return;
        }
        clear_other_files(&inner.log_dir, &inner.base_path, &inner.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log_dir() -> PathBuf {
        std::env::temp_dir().join(format!(
            "agentbuddy-route-log-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn daily_path_appends_date_before_extension() {
        let base = Path::new("/tmp/logs/route_aggregation.log");
        let date = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
        assert_eq!(
            daily_path(base, date),
            PathBuf::from("/tmp/logs/route_aggregation_2026-08-15.log")
        );
    }

    #[test]
    fn cleanup_keeps_seven_calendar_days_and_ignores_unrelated_files() {
        let dir = temp_log_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("route_aggregation.log");
        let today = NaiveDate::from_ymd_opt(2026, 8, 15).unwrap();
        for offset in 0..8 {
            let date = today - chrono::Duration::days(offset);
            std::fs::write(daily_path(&base, date), b"log").unwrap();
        }
        std::fs::write(dir.join("route_aggregation_notes.log"), b"keep").unwrap();

        cleanup_old_files(&dir, &base, today);

        assert!(!daily_path(&base, today - chrono::Duration::days(7)).exists());
        assert!(daily_path(&base, today - chrono::Duration::days(6)).exists());
        assert!(dir.join("route_aggregation_notes.log").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn open_uses_current_daily_file() {
        let dir = temp_log_dir();
        let base = dir.join("route_aggregation.log");
        let log = LogFile::open(&base).unwrap();
        assert_eq!(log.path(), daily_path(&base, Local::now().date_naive()));
        assert!(log.path().starts_with(&dir));
        drop(log);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
