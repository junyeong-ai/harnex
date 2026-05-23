//! # Path guards and safe write primitives
//!
//! Every file mutation in the toolkit routes through [`write_atomic`]
//! (full-file replace) or [`append_line`] (append-only ledgers).
//! Direct `std::fs::write` / `File::create + write_all` in domain modules
//! is forbidden — the test suite asserts the call shape.
//!
//! ## What this module refuses to do
//!
//! - Never follow symlinks on write (reads MAY follow them so the scanner
//!   indexes linked content).
//! - Never accept paths containing `..` segments — those open the door to
//!   writing outside the project root.
//! - Never write non-atomically for full-file replace. A partial write
//!   leaving a half-formed file would corrupt the consumer's state.
//!   Append-only ledgers use [`append_line`], which is inherently
//!   incremental and does not need temp-file atomicity.

use std::fs;
use std::io::Write;
use std::path::{Component, Path};

use crate::error::{Error, Result};

/// Reject any path that contains a `..` segment.
///
/// The caller is responsible for resolving the path relative to a known
/// root before passing it here — this guard only prevents escape via
/// parent-dir components, not via absolute paths (which the caller has
/// presumably already authorised).
pub fn reject_traversal(path: &Path) -> Result<()> {
    for component in path.components() {
        if component == Component::ParentDir {
            return Err(Error::PathTraversal {
                path: path.to_path_buf(),
            });
        }
    }
    Ok(())
}

/// Reject overwriting a symbolic link.
pub fn reject_symlink_write(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Error::PathSymlinkRefused {
            path: path.to_path_buf(),
        }),
        _ => Ok(()),
    }
}

/// Atomically write `contents` to `path`.
///
/// Writes through a same-directory temp file followed by `rename`, so a
/// crash mid-write cannot leave a partial file at `path`. Parent
/// directories are created as needed. Symlink targets are refused.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    reject_traversal(path)?;
    reject_symlink_write(path)?;

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    fs::create_dir_all(parent).map_err(|e| Error::IoFailure {
        path: parent.to_path_buf(),
        source: e,
    })?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent).map_err(|e| Error::IoFailure {
        path: parent.to_path_buf(),
        source: e,
    })?;

    tmp.write_all(contents).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;

    tmp.persist(path).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e.error,
    })?;

    Ok(())
}

/// Append `line` (with trailing newline) to `path`.
///
/// Applies the same safety guards as [`write_atomic`] (traversal
/// rejection + symlink-write rejection) but uses append semantics
/// instead of atomic replace. Parent directories are created as needed.
pub fn append_line(path: &Path, line: &[u8]) -> Result<()> {
    reject_traversal(path)?;
    reject_symlink_write(path)?;

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };

    fs::create_dir_all(parent).map_err(|e| Error::IoFailure {
        path: parent.to_path_buf(),
        source: e,
    })?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Error::IoFailure {
            path: path.to_path_buf(),
            source: e,
        })?;

    file.write_all(line).map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;
    file.write_all(b"\n").map_err(|e| Error::IoFailure {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn rejects_parent_dir_segment() {
        let path = PathBuf::from("foo/../bar");
        assert!(matches!(
            reject_traversal(&path).unwrap_err(),
            Error::PathTraversal { .. }
        ));
    }

    #[test]
    fn rejects_leading_parent_dir() {
        let path = PathBuf::from("../bar");
        assert!(reject_traversal(&path).is_err());
    }

    #[test]
    fn accepts_plain_relative() {
        let path = PathBuf::from("foo/bar/baz.txt");
        assert!(reject_traversal(&path).is_ok());
    }

    #[test]
    fn accepts_absolute_no_dotdot() {
        // The guard only blocks `..`; absolute paths are caller's call.
        let path = PathBuf::from("/tmp/foo");
        assert!(reject_traversal(&path).is_ok());
    }

    #[test]
    fn write_atomic_creates_parent_and_writes() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("a/b/c.txt");
        write_atomic(&target, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    fn write_atomic_refuses_symlink() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.txt");
        fs::write(&real, "real").unwrap();
        let link = tmp.path().join("link.txt");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&real, &link).unwrap();
        assert!(matches!(
            write_atomic(&link, b"x").unwrap_err(),
            Error::PathSymlinkRefused { .. }
        ));
    }

    #[test]
    fn write_atomic_refuses_dotdot() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("a/../b.txt");
        let err = write_atomic(&target, b"x").unwrap_err();
        assert!(matches!(err, Error::PathTraversal { .. }));
    }

    #[test]
    fn append_line_creates_and_appends() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("ledger.jsonl");
        append_line(&target, b"line1").unwrap();
        append_line(&target, b"line2").unwrap();
        let content = fs::read_to_string(&target).unwrap();
        assert_eq!(content, "line1\nline2\n");
    }

    #[test]
    fn append_line_refuses_symlink() {
        let tmp = TempDir::new().unwrap();
        let real = tmp.path().join("real.jsonl");
        fs::write(&real, "real").unwrap();
        let link = tmp.path().join("link.jsonl");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&real, &link).unwrap();
        assert!(matches!(
            append_line(&link, b"x").unwrap_err(),
            Error::PathSymlinkRefused { .. }
        ));
    }

    #[test]
    fn append_line_refuses_dotdot() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("a/../b.jsonl");
        let err = append_line(&target, b"x").unwrap_err();
        assert!(matches!(err, Error::PathTraversal { .. }));
    }

    #[test]
    fn append_line_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("a/b/c.jsonl");
        append_line(&target, b"hello").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "hello\n");
    }
}
