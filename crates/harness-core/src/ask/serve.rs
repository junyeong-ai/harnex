//! One decision page served on 127.0.0.1 until one answer set is taken, a
//! source turns out to have changed, or the time given runs out.

use std::io::{ErrorKind, Read};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use jiff::Timestamp;

use super::answers::{self, Outcome};
use super::asks::Asks;
use super::files::Site;
use super::http::{self, BODY_LIMIT, HEAD_LIMIT, Parsed, Request, Status};
use crate::error::{Error, Result};

const SCRIPT: &str = include_str!("script.js");

/// How long a connection may take to send its request. A browser opens
/// connections it may never use, and one of those must not hold the page.
const IDLE: Duration = Duration::from_secs(10);
const WRITE_LIMIT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(10);
const OPENER_WAIT: Duration = Duration::from_secs(5);

/// Opens an address in the person's browser, or says why it could not.
pub trait Browser {
    fn open(&self, url: &str) -> std::result::Result<(), String>;
}

/// The platform's own opener, which hands an address to the default browser.
pub struct SystemBrowser;

impl Browser for SystemBrowser {
    fn open(&self, url: &str) -> std::result::Result<(), String> {
        let opener = match std::env::consts::OS {
            "macos" => "open",
            "linux" => "xdg-open",
            other => return Err(format!("no browser opener is known on {other}")),
        };
        let mut child = Command::new(opener)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{opener}: {e}"))?;
        let until = Instant::now() + OPENER_WAIT;
        loop {
            match child.try_wait() {
                Ok(Some(status)) if status.success() => return Ok(()),
                Ok(Some(status)) => return Err(format!("{opener} exited with {status}")),
                // An opener that runs the browser in the foreground is still
                // running once the page is up, and one that fails exits. So one
                // still running is taken as having opened the page; one hanging
                // without opening anything leaves the ask unanswered at its
                // deadline, with the address already printed.
                Ok(None) if Instant::now() >= until => return Ok(()),
                Ok(None) => std::thread::sleep(POLL),
                Err(e) => return Err(format!("{opener}: {e}")),
            }
        }
    }
}

/// What to serve, and for how long.
pub struct Serving<'a> {
    pub page: &'a Path,
    pub asks: &'a Asks,
    /// What the asks file's `sources` resolve against.
    pub base: &'a Path,
    pub within: Duration,
}

/// Serve the page until it settles, then stop. `announce` hears the address
/// the moment the page is served; `browser`, when given, is asked to open it,
/// and nothing is served past its refusal.
pub fn serve(
    serving: &Serving<'_>,
    browser: Option<&dyn Browser>,
    announce: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let site = Site::of(serving.page)?;
    let sources = read_sources(serving.asks, serving.base)?;
    let token = token()?;
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|source| Error::AskListenFailed { source })?;
    let port = listener
        .local_addr()
        .map_err(|source| Error::AskListenFailed { source })?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|source| Error::AskListenFailed { source })?;

    let deadline = Instant::now() + serving.within;
    let closes =
        Timestamp::now()
            .checked_add(serving.within)
            .map_err(|e| Error::AskInputInvalid {
                path: serving.page.to_path_buf(),
                message: format!("the time given runs past what a timestamp holds: {e}"),
            })?;
    let url = format!("http://127.0.0.1:{port}/{token}/page/{}", site.page_path());
    let server = Server {
        prefix: format!("/{token}/"),
        host: format!("127.0.0.1:{port}"),
        origin: format!("http://127.0.0.1:{port}"),
        tag: format!("\n<script src=\"/{token}/ask.js\"></script>\n"),
        script: script(serving.asks, &token, closes),
        site,
        asks: serving.asks,
        sources,
        url: url.clone(),
    };

    announce(&url);
    if let Some(browser) = browser {
        browser
            .open(&url)
            .map_err(|reason| Error::AskBrowserUnopened {
                url: url.clone(),
                reason,
            })?;
    }

    let mut open: Vec<Connection> = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Ok(Outcome::Unanswered { url });
        }
        let mut moved = false;
        loop {
            match listener.accept() {
                Ok((stream, _)) => {
                    moved = true;
                    if stream.set_nonblocking(true).is_ok() {
                        open.push(Connection {
                            stream,
                            read: Vec::new(),
                            since: Instant::now(),
                        });
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e)
                    if matches!(
                        e.kind(),
                        ErrorKind::ConnectionAborted | ErrorKind::Interrupted
                    ) => {}
                Err(source) => return Err(Error::AskListenFailed { source }),
            }
        }
        let mut i = 0;
        while i < open.len() {
            match open[i].pump() {
                Pumped::Waiting if open[i].since.elapsed() < IDLE => i += 1,
                Pumped::Waiting | Pumped::Gone => {
                    open.swap_remove(i);
                }
                Pumped::Parsed(parsed) => {
                    moved = true;
                    let connection = open.swap_remove(i);
                    let (reply, settled) = match parsed {
                        Parsed::Whole(request) => server.handle(&request),
                        Parsed::Refused(status) => (server.refused(status), None),
                        Parsed::Partial => unreachable!("a partial request is still waiting"),
                    };
                    connection.send(&reply);
                    if let Some(outcome) = settled {
                        return Ok(outcome);
                    }
                }
            }
        }
        if !moved {
            std::thread::sleep(POLL);
        }
    }
}

fn read_sources(asks: &Asks, base: &Path) -> Result<Vec<(PathBuf, Vec<u8>)>> {
    asks.sources
        .iter()
        .map(|source| {
            let path = base.join(source);
            std::fs::read(&path)
                .map(|bytes| (path.clone(), bytes))
                .map_err(|source| Error::IoFailure { path, source })
        })
        .collect()
}

/// 128 bits from the operating system, as the path nothing else knows.
fn token() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| Error::IoFailure {
        path: PathBuf::from("(operating system random source)"),
        source: std::io::Error::other(e.to_string()),
    })?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn script(asks: &Asks, token: &str, closes: Timestamp) -> String {
    let setting = serde_json::json!({
        "endpoint": format!("/{token}/answers"),
        "asks": asks,
        "words": asks.locale.words(),
        "deadline": closes.to_string(),
    });
    format!("const ASK = {setting};\n{SCRIPT}")
}

struct Reply {
    status: Status,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Reply {
    fn json(status: Status, value: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: value.to_string().into_bytes(),
        }
    }

    fn empty(status: Status) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        }
    }
}

struct Server<'a> {
    prefix: String,
    host: String,
    origin: String,
    tag: String,
    script: String,
    site: Site,
    asks: &'a Asks,
    sources: Vec<(PathBuf, Vec<u8>)>,
    url: String,
}

impl Server<'_> {
    /// A page on another site can reach a loopback port through a name it
    /// points there, and such a request carries that name as its host; it is
    /// refused before anything else is read.
    fn handle(&self, request: &Request) -> (Reply, Option<Outcome>) {
        if request.host.as_deref() != Some(self.host.as_str()) {
            return (Reply::empty(Status::Forbidden), None);
        }
        let Some(rest) = request.path.strip_prefix(&self.prefix) else {
            return (Reply::empty(Status::NotFound), None);
        };
        match (request.method.as_str(), rest) {
            ("GET", "ask.js") => (
                Reply {
                    status: Status::Ok,
                    content_type: "text/javascript; charset=utf-8",
                    body: self.script.clone().into_bytes(),
                },
                None,
            ),
            ("GET", path) => (self.file(path), None),
            ("POST", "answers") => self.answers(request),
            _ => (Reply::empty(Status::NotFound), None),
        }
    }

    fn file(&self, path: &str) -> Reply {
        let Some(file) = path
            .strip_prefix("page/")
            .and_then(|path| self.site.file(path))
        else {
            return Reply::empty(Status::NotFound);
        };
        let Ok(mut body) = std::fs::read(&file.path) else {
            return Reply::empty(Status::NotFound);
        };
        if file.is_page {
            body.extend_from_slice(self.tag.as_bytes());
        }
        Reply {
            status: Status::Ok,
            content_type: file.content_type,
            body,
        }
    }

    /// Only the page this server served posts from its origin; a form on
    /// another site that reaches the port carries that site's.
    fn answers(&self, request: &Request) -> (Reply, Option<Outcome>) {
        if request.origin.as_deref() != Some(self.origin.as_str()) {
            return (Reply::empty(Status::Forbidden), None);
        }
        let words = self.asks.locale.words();
        if self.sources_moved() {
            return (
                Reply::json(
                    Status::Conflict,
                    serde_json::json!({ "problem": words.stale, "final": true }),
                ),
                Some(Outcome::Stale {
                    url: self.url.clone(),
                }),
            );
        }
        match answers::read(&request.body, self.asks, Timestamp::now()) {
            Err(refusal) => (
                Reply::json(
                    Status::BadRequest,
                    serde_json::json!({ "problem": refusal.said(words) }),
                ),
                None,
            ),
            Ok(answers) => (
                Reply::json(Status::Ok, serde_json::json!({ "final": true })),
                Some(Outcome::Answered {
                    url: self.url.clone(),
                    answers,
                }),
            ),
        }
    }

    fn sources_moved(&self) -> bool {
        self.sources
            .iter()
            .any(|(path, bytes)| std::fs::read(path).ok().as_ref() != Some(bytes))
    }

    fn refused(&self, status: Status) -> Reply {
        let words = self.asks.locale.words();
        let problem = match status {
            Status::PayloadTooLarge => words.too_large,
            _ => words.unreadable,
        };
        Reply::json(status, serde_json::json!({ "problem": problem }))
    }
}

struct Connection {
    stream: TcpStream,
    read: Vec<u8>,
    since: Instant,
}

enum Pumped {
    Waiting,
    Gone,
    Parsed(Parsed),
}

impl Connection {
    /// Read what the connection has sent so far, without waiting for more.
    fn pump(&mut self) -> Pumped {
        let mut chunk = [0u8; 8192];
        loop {
            match self.stream.read(&mut chunk) {
                Ok(0) => {
                    return match http::parse(&self.read) {
                        Parsed::Partial => Pumped::Gone,
                        parsed => Pumped::Parsed(parsed),
                    };
                }
                Ok(n) => {
                    self.read.extend_from_slice(&chunk[..n]);
                    if self.read.len() > HEAD_LIMIT + BODY_LIMIT {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Pumped::Gone,
            }
        }
        match http::parse(&self.read) {
            Parsed::Partial => Pumped::Waiting,
            parsed => Pumped::Parsed(parsed),
        }
    }

    /// A client that leaves before its reply is written loses only the reply:
    /// what it sent has already been taken or refused.
    fn send(mut self, reply: &Reply) {
        if self.stream.set_nonblocking(false).is_err()
            || self.stream.set_write_timeout(Some(WRITE_LIMIT)).is_err()
        {
            return;
        }
        let _ = http::respond(
            &mut self.stream,
            reply.status,
            reply.content_type,
            &reply.body,
        );
    }
}
