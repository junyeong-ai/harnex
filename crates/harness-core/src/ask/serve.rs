//! One decision page served on 127.0.0.1 until one answer set is taken, a
//! source turns out to have changed, or the time given runs out.

use std::io::{ErrorKind, Read, Write};
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

/// What one connection may hold of a loop that serves them all in turn.
struct Limits {
    /// How long a connection may go without moving: sending its request from
    /// when it was taken, or taking any of its reply. A browser opens
    /// connections it may never use, and stops taking a reply it no longer
    /// wants, such as a paused video.
    idle: Duration,
    /// How many connections are held at once; the rest wait in the listen
    /// backlog. A browser keeps a handful to one host, and anything on this
    /// machine can open more: holding every one would let them run the process
    /// out of descriptors, or arrive faster than they are taken and keep the
    /// loop from its deadline.
    connections: usize,
}

const LIMITS: Limits = Limits {
    idle: Duration::from_secs(10),
    connections: 64,
};
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
    serve_under(serving, browser, announce, &LIMITS)
}

fn serve_under(
    serving: &Serving<'_>,
    browser: Option<&dyn Browser>,
    announce: &mut dyn FnMut(&str),
    limits: &Limits,
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

    // Nothing here waits on one connection: each is read and written as far
    // as it goes without blocking, so a reader that stops cannot hold the
    // others or the deadline. Once an answer set is taken the rest are
    // dropped, and the command ends when its reply is written or cannot be.
    let mut open: Vec<Connection> = Vec::new();
    let mut settled: Option<Outcome> = None;
    loop {
        if Instant::now() >= deadline {
            return Ok(settled.unwrap_or(Outcome::Unanswered { url }));
        }
        let mut moved = settled.is_none() && admit(&listener, &mut open, limits)?;
        let mut i = 0;
        while i < open.len() {
            let connection = &mut open[i];
            let keep = if settled.is_some() && !connection.settles {
                false
            } else {
                match &mut connection.stage {
                    Stage::Receiving(received) => match receive(&mut connection.stream, received) {
                        Received::Waiting => connection.since.elapsed() < limits.idle,
                        Received::Gone => false,
                        Received::Request(parsed) => {
                            moved = true;
                            let (reply, outcome) = match parsed {
                                Parsed::Whole(request) => server.handle(&request),
                                Parsed::Refused(status) => (server.refused(status), None),
                                Parsed::Partial => {
                                    unreachable!("a partial request is still being received")
                                }
                            };
                            connection.settles = outcome.is_some();
                            settled = outcome;
                            connection.stage = Stage::Replying {
                                reply: http::response(
                                    reply.status,
                                    reply.content_type,
                                    &reply.body,
                                ),
                                sent: 0,
                            };
                            connection.since = Instant::now();
                            true
                        }
                    },
                    Stage::Replying { reply, sent } => {
                        match send(&mut connection.stream, reply, sent) {
                            Sent::Part => {
                                moved = true;
                                connection.since = Instant::now();
                                true
                            }
                            Sent::Nothing => connection.since.elapsed() < limits.idle,
                            Sent::All => {
                                moved = true;
                                false
                            }
                            Sent::Gone => false,
                        }
                    }
                }
            };
            if keep {
                i += 1;
            } else {
                open.swap_remove(i);
            }
        }
        if open.is_empty()
            && let Some(outcome) = settled.take()
        {
            return Ok(outcome);
        }
        if !moved {
            std::thread::sleep(POLL);
        }
    }
}

/// Take the connections waiting on `listener`, as many as the bound leaves
/// room for; the rest wait in the listen backlog. Whether any was taken.
fn admit(listener: &TcpListener, open: &mut Vec<Connection>, limits: &Limits) -> Result<bool> {
    let mut moved = false;
    while open.len() < limits.connections {
        match listener.accept() {
            Ok((stream, _)) => {
                moved = true;
                if stream.set_nonblocking(true).is_ok() {
                    open.push(Connection {
                        stream,
                        stage: Stage::Receiving(Vec::new()),
                        since: Instant::now(),
                        settles: false,
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
    Ok(moved)
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
    stage: Stage,
    /// When it last moved: when it was taken, or when the socket last took
    /// part of its reply.
    since: Instant,
    /// Whether its reply answers the answer set that was taken.
    settles: bool,
}

enum Stage {
    /// What the client has sent, until its request is whole.
    Receiving(Vec<u8>),
    /// The response, and how much of it the socket has taken.
    Replying { reply: Vec<u8>, sent: usize },
}

enum Received {
    Waiting,
    Gone,
    Request(Parsed),
}

enum Sent {
    Part,
    Nothing,
    All,
    Gone,
}

/// Read what the client has sent so far, without waiting for more.
fn receive(stream: &mut TcpStream, received: &mut Vec<u8>) -> Received {
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => {
                return match http::parse(received) {
                    Parsed::Partial => Received::Gone,
                    parsed => Received::Request(parsed),
                };
            }
            Ok(n) => {
                received.extend_from_slice(&chunk[..n]);
                if received.len() > HEAD_LIMIT + BODY_LIMIT {
                    break;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Received::Gone,
        }
    }
    match http::parse(received) {
        Parsed::Partial => Received::Waiting,
        parsed => Received::Request(parsed),
    }
}

/// Write as much of `reply` past `sent` as the socket takes, without waiting.
/// A client that leaves before its reply is written loses only the reply:
/// what it sent has already been taken or refused.
fn send(stream: &mut TcpStream, reply: &[u8], sent: &mut usize) -> Sent {
    let before = *sent;
    while *sent < reply.len() {
        match stream.write(&reply[*sent..]) {
            Ok(0) => return Sent::Gone,
            Ok(n) => *sent += n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Sent::Gone,
        }
    }
    if *sent == reply.len() {
        Sent::All
    } else if *sent > before {
        Sent::Part
    } else {
        Sent::Nothing
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread::JoinHandle;

    use super::*;

    const ASKS: &str = r#"{"asks": [{"id": "d-1", "label": "결정", "version": "v1", "answers": [
        {"name": "a", "note": "none"}, {"name": "b", "note": "none"}]}]}"#;

    struct Served {
        port: u16,
        token: String,
        ended: JoinHandle<Result<Outcome>>,
        _dir: tempfile::TempDir,
    }

    /// A page beside a file larger than any socket buffer holds, served under
    /// `limits` for `within`.
    fn start(within: Duration, limits: Limits) -> Served {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("page.html"), "<p>x</p>").unwrap();
        std::fs::write(dir.path().join("large.bin"), vec![0u8; 32 << 20]).unwrap();
        let root = dir.path().to_path_buf();
        let (heard, hear) = mpsc::channel();
        let ended = std::thread::spawn(move || {
            let asks = Asks::parse(ASKS).unwrap();
            let serving = Serving {
                page: &root.join("page.html"),
                asks: &asks,
                base: &root,
                within,
            };
            serve_under(
                &serving,
                None,
                &mut |url| heard.send(url.to_string()).unwrap(),
                &limits,
            )
        });
        let url = hear.recv_timeout(Duration::from_secs(10)).unwrap();
        let (port, path) = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split_once('/'))
            .unwrap();
        Served {
            port: port.parse().unwrap(),
            token: path.split('/').next().unwrap().to_string(),
            ended,
            _dir: dir,
        }
    }

    impl Served {
        fn ask(&self, path: &str) -> TcpStream {
            let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
            write!(
                stream,
                "GET /{}/{path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
                self.token, self.port
            )
            .unwrap();
            stream
        }

        fn status(&self, path: &str) -> String {
            let mut stream = self.ask(path);
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut head = [0u8; 12];
            stream.read_exact(&mut head).unwrap();
            String::from_utf8_lossy(&head).into_owned()
        }
    }

    #[test]
    fn a_reply_left_unread_holds_neither_the_page_nor_the_deadline() {
        let within = Duration::from_secs(2);
        let started = Instant::now();
        let served = start(within, LIMITS);
        let _unread = served.ask("page/large.bin");
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(served.status("page/page.html"), "HTTP/1.1 200");
        let outcome = served.ended.join().unwrap().unwrap();
        assert!(matches!(outcome, Outcome::Unanswered { .. }), "{outcome:?}");
        assert!(
            started.elapsed() < within + Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_connection_that_stops_moving_gives_up_its_place() {
        let limits = Limits {
            idle: Duration::from_millis(300),
            connections: 1,
        };
        let served = start(Duration::from_secs(5), limits);
        let _silent = TcpStream::connect(("127.0.0.1", served.port)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            served.status("page/page.html"),
            "HTTP/1.1 200",
            "past one that sends nothing"
        );
        let _unread = served.ask("page/large.bin");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            served.status("page/page.html"),
            "HTTP/1.1 200",
            "past one that takes nothing"
        );
    }

    #[test]
    fn connections_past_the_bound_wait_in_the_backlog() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let bound = LIMITS.connections;
        let _clients: Vec<TcpStream> = (0..bound + 8)
            .map(|_| TcpStream::connect(("127.0.0.1", port)).unwrap())
            .collect();
        let mut open = Vec::new();
        assert!(admit(&listener, &mut open, &LIMITS).unwrap());
        assert_eq!(open.len(), bound);
        assert!(!admit(&listener, &mut open, &LIMITS).unwrap());

        open.truncate(bound - 8);
        assert!(admit(&listener, &mut open, &LIMITS).unwrap());
        assert_eq!(open.len(), bound);
    }
}
