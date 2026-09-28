//! The page's directory, served read-only, so a page keeps the stylesheets,
//! images and fonts it names by relative path.

use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

use crate::error::{Error, Result};

/// What a path segment keeps unencoded: the unreserved characters.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

#[derive(Debug)]
pub struct Site {
    root: PathBuf,
    page: PathBuf,
    name: String,
}

/// A file the site serves.
#[derive(Debug, PartialEq, Eq)]
pub struct File {
    pub path: PathBuf,
    /// Whether it is the page the transport's script is set into.
    pub is_page: bool,
    pub content_type: &'static str,
}

impl Site {
    /// The site of the page at `page`: its directory, as the file system
    /// resolves it.
    pub fn of(page: &Path) -> Result<Self> {
        let io = |source| Error::IoFailure {
            path: page.to_path_buf(),
            source,
        };
        let page = std::fs::canonicalize(page).map_err(io)?;
        if !page.is_file() {
            return Err(Error::AskInputInvalid {
                path: page,
                message: "the page is not a file".into(),
            });
        }
        let invalid = |message: &str| Error::AskInputInvalid {
            path: page.clone(),
            message: message.into(),
        };
        let name = page
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| invalid("the page's file name is not UTF-8"))?
            .to_string();
        let root = page
            .parent()
            .ok_or_else(|| invalid("the page has no directory"))?
            .to_path_buf();
        Ok(Self { root, page, name })
    }

    /// The page's path below the site, encoded for a URL.
    pub fn page_path(&self) -> String {
        utf8_percent_encode(&self.name, SEGMENT).to_string()
    }

    /// The file `path` (below the site, as a URL spells it) names, if it is
    /// one the site serves: inside the page's directory once links are
    /// followed, and reached through no hidden name. The second keeps `.git`
    /// and `.env` unserved when the page sits at a repository root; a page
    /// that links an asset under a dot-directory cannot load it.
    pub fn file(&self, path: &str) -> Option<File> {
        let mut candidate = self.root.clone();
        for segment in path.split('/') {
            let segment = percent_decode_str(segment).decode_utf8().ok()?;
            if segment.is_empty() || segment.starts_with('.') || segment.contains(['/', '\\', '\0'])
            {
                return None;
            }
            candidate.push(segment.as_ref());
        }
        let resolved = std::fs::canonicalize(&candidate).ok()?;
        if !resolved.starts_with(&self.root) || !resolved.is_file() {
            return None;
        }
        Some(File {
            is_page: resolved == self.page,
            content_type: content_type(&resolved),
            path: resolved,
        })
    }
}

fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> (tempfile::TempDir, Site) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("pages");
        std::fs::create_dir_all(root.join("styles")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("설계 1.html"), "<p>x</p>").unwrap();
        std::fs::write(root.join("styles/base.css"), "p{}").unwrap();
        std::fs::write(root.join(".git/config"), "secret").unwrap();
        std::fs::write(dir.path().join("outside.txt"), "outside").unwrap();
        let site = Site::of(&root.join("설계 1.html")).unwrap();
        (dir, site)
    }

    #[test]
    fn the_page_and_what_it_links_are_served() {
        let (_dir, site) = site();
        let page = site.file(&site.page_path()).unwrap();
        assert!(page.is_page);
        assert_eq!(page.content_type, "text/html; charset=utf-8");
        let css = site.file("styles/base.css").unwrap();
        assert!(!css.is_page);
        assert_eq!(css.content_type, "text/css; charset=utf-8");
        assert_eq!(site.page_path(), "%EC%84%A4%EA%B3%84%201.html");
    }

    #[test]
    fn nothing_outside_the_directory_or_hidden_is_served() {
        let (dir, site) = site();
        for path in [
            "../outside.txt",
            "%2E%2E/outside.txt",
            "styles/..%2F..%2Foutside.txt",
            ".git/config",
            "%2Egit/config",
            "styles//base.css",
            "",
            "styles",
            "missing.css",
            "%FF",
        ] {
            assert_eq!(site.file(path), None, "{path}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                dir.path().join("outside.txt"),
                dir.path().join("pages/link.txt"),
            )
            .unwrap();
            assert_eq!(site.file("link.txt"), None, "a link leaving the directory");
        }
    }
}
