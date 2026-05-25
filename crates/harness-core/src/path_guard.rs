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

/// Reject overwriting a symlink at the leaf — never clobber a symlinked
/// file. `symlink_metadata` does not follow the final component, so this
/// sees the leaf's own type.
///
/// SCOPE (deliberate): this guards the leaf only. Symlinked *ancestor*
/// directories (`link/file`, or a planted `.harness -> /outside` above the
/// leaf) are NOT policed here. Two earlier attempts to extend coverage were
/// both unsound: a full root-down walk false-rejects legitimate paths under
/// system mount symlinks (macOS `/var -> /private/var`), and an
/// immediate-parent check both false-rejects those AND misses the realistic
/// case — the toolkit writes to `<root>/.harness/<sub>/<file>`, so a planted
/// symlink at `.harness` sits two levels above the leaf. Covering ancestors
/// soundly requires a trusted-root anchor (canonicalize-and-compare, or
/// no-follow component opens) threaded from each caller.
///
/// That is out of this tool's threat model: `harness` is a local,
/// single-user, no-network CLI operating on the user's own repository. An
/// attacker who can plant a symlink inside the working tree already has write
/// access to it — at which point the repository is compromised regardless of
/// where the tool's own (benign, tool-authored) ledger writes land. The
/// meaningful, in-contract guarantee is leaf-overwrite refusal plus `..`
/// rejection ([`reject_traversal`]); callers that derive a filename from
/// input additionally pin it to a single component (see telemetry append).
pub fn reject_symlink_write(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        return Err(Error::PathSymlinkRefused {
            path: path.to_path_buf(),
        });
    }
    Ok(())
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

    // Assemble line + newline into ONE buffer and write once. Two separate
    // write_all calls let concurrent appenders interleave a record and its
    // newline, corrupting the JSONL ledger. A single append-mode write of a
    // record-sized buffer is atomic on POSIX.
    let mut record = Vec::with_capacity(line.len() + 1);
    record.extend_from_slice(line);
    record.push(b'\n');
    file.write_all(&record).map_err(|e| Error::IoFailure {
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

    #[test]
    fn write_atomic_allows_leaf_under_system_symlink_parent() {
        // A leaf whose immediate parent is a (legitimate) symlinked directory
        // must NOT be refused — only the leaf's own symlink-ness matters.
        // This is the false-positive the prior immediate-parent check caused
        // for paths like `/tmp/x` / `/var/x` on macOS.
        let tmp = TempDir::new().unwrap();
        let real_dir = tmp.path().join("real_dir");
        fs::create_dir(&real_dir).unwrap();
        let link_dir = tmp.path().join("link_dir");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&real_dir, &link_dir).unwrap();
        // Writing a NEW file under the symlinked parent is allowed.
        write_atomic(&link_dir.join("file.txt"), b"x").unwrap();
        assert_eq!(fs::read_to_string(real_dir.join("file.txt")).unwrap(), "x");
    }

    #[test]
    fn append_line_writes_record_atomically() {
        // The record and its newline are written in a single buffer, so a
        // reader never observes a record without its terminating newline.
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("ledger.jsonl");
        append_line(&target, br#"{"a":1}"#).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"{\"a\":1}\n");
    }
}
