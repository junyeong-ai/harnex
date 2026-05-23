//! # JSONL telemetry storage
//!
//! One `.jsonl` file per Kind under the configured directory. Each line
//! is a single [`super::Event`] serialised as JSON. Rotation renames the
//! active file with a timestamp suffix when it crosses the configured
//! size threshold; old files are kept (retention is a separate sweep).

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::path_guard;

use super::Event;

/// Acquire an exclusive advisory lock on a sibling `.lock` file, scoped
/// to the lifetime of the returned [`File`]. Used to serialise the
/// append + rotation critical section across concurrent processes —
/// without it, two CLI invocations writing the same Kind ledger could
/// double-rotate or interleave bytes mid-write.
///
/// We lock a sibling file (`<path>.lock`) rather than the data file
/// itself because rotation renames the data file mid-section, which
/// would invalidate a lock held on the renamed inode.
fn acquire_lock(data_path: &Path) -> Result<File> {
    let lock_path = data_path.with_extension("jsonl.lock");
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|e| Error::IoFailure {
            path: lock_path.clone(),
            source: e,
        })?;
    lock.lock().map_err(|e| Error::IoFailure {
        path: lock_path,
        source: e,
    })?;
    Ok(lock)
}

pub struct JsonlStorage {
    dir: PathBuf,
    rotate_at_bytes: u64,
}

impl JsonlStorage {
    pub fn new(dir: PathBuf, rotate_at_mb: u32) -> Self {
        Self {
            dir,
            rotate_at_bytes: (rotate_at_mb as u64) * 1024 * 1024,
        }
    }

    fn current_file(&self, kind: &str) -> PathBuf {
        self.dir.join(format!("{kind}.jsonl"))
    }

    fn rotate_if_needed(&self, path: &Path) -> Result<()> {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => 0,
        };
        if size >= self.rotate_at_bytes && size > 0 {
            let ts = jiff::Timestamp::now()
                .strftime("%Y%m%dT%H%M%S")
                .to_string();
            let parent = path.parent().unwrap_or(Path::new("."));
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("rotated");
            let rotated = parent.join(format!("{stem}-{ts}.jsonl"));
            std::fs::rename(path, &rotated).map_err(|e| Error::IoFailure {
                path: path.to_path_buf(),
                source: e,
            })?;
        }
        Ok(())
    }
}

impl JsonlStorage {
    /// Append a telemetry event to the Kind-specific JSONL ledger.
    ///
    /// Uses `path_guard` safety guards (traversal + symlink rejection)
    /// directly rather than via [`path_guard::append_line`] because the
    /// advisory lock must be held across the rotate-and-append critical
    /// section, which requires an open file handle incompatible with
    /// `append_line`'s self-contained open-write-close cycle.
    pub fn append(&mut self, event: &Event) -> Result<()> {
        path_guard::reject_traversal(&self.dir)?;
        std::fs::create_dir_all(&self.dir).map_err(|e| Error::IoFailure {
            path: self.dir.clone(),
            source: e,
        })?;

        let path = self.current_file(&event.kind);
        path_guard::reject_symlink_write(&path)?;

        // Serialise the rotate-and-append critical section across processes.
        // The lock is dropped (released) when this function returns.
        let _lock = acquire_lock(&path)?;
        self.rotate_if_needed(&path)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;

        let line = serde_json::to_string(event).map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e),
        })?;

        writeln!(file, "{line}").map_err(|e| Error::IoFailure {
            path: path.clone(),
            source: e,
        })?;
        Ok(())
    }

    pub fn scan(&self, visitor: &mut dyn FnMut(&Event)) -> Result<()> {
        if !self.dir.exists() {
            return Ok(());
        }
        let entries = std::fs::read_dir(&self.dir).map_err(|e| Error::IoFailure {
            path: self.dir.clone(),
            source: e,
        })?;
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
            .collect();
        paths.sort();
        for path in paths {
            let file = File::open(&path).map_err(|e| Error::IoFailure {
                path: path.clone(),
                source: e,
            })?;
            let reader = BufReader::new(file);
            for line in reader.lines().map_while(std::result::Result::ok) {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<Event>(&line) {
                    visitor(&event);
                }
            }
        }
        Ok(())
    }
}
