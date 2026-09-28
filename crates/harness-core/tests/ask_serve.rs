//! `ask::serve` over real sockets: what a browser on this machine is served,
//! what anything else reaching the port is refused, and how the wait ends.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use harness_core::ask::answers::Outcome;
use harness_core::ask::asks::Asks;
use harness_core::ask::serve::{Browser, Serving, serve};
use harness_core::{Error, ErrorCode};

const ASKS: &str = r#"{
    "locale": "ko",
    "sources": ["decision.md"],
    "asks": [
        {"id": "d-1", "label": "결정 1", "version": "v1", "answers": [
            {"name": "지금 만든다", "note": "none"},
            {"name": "고칠 곳이 있다", "note": "required"}]}
    ]
}"#;

const PAGE: &str = "<!doctype html><title>t</title><fieldset data-ask=\"d-1\" data-version=\"v1\"></fieldset><div data-ask-send>파일로 열렸다</div>";

struct Served {
    dir: tempfile::TempDir,
    port: u16,
    token: String,
    waiting: JoinHandle<harness_core::Result<Outcome>>,
}

impl Served {
    fn start(within: Duration, browser: Option<Box<dyn Browser + Send>>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("site/styles")).unwrap();
        std::fs::create_dir_all(root.join("site/.git")).unwrap();
        std::fs::write(root.join("site/page.html"), PAGE).unwrap();
        std::fs::write(root.join("site/styles/base.css"), "p{}").unwrap();
        std::fs::write(root.join("site/.git/config"), "secret").unwrap();
        std::fs::write(root.join("decision.md"), "# 결정\n").unwrap();
        let (tell, heard) = mpsc::channel();
        let waiting = std::thread::spawn(move || {
            let asks = Asks::parse(ASKS).unwrap();
            let page: PathBuf = root.join("site/page.html");
            let serving = Serving {
                page: &page,
                asks: &asks,
                base: &root,
                within,
            };
            serve(
                &serving,
                browser.as_deref().map(|b| b as &dyn Browser),
                &mut |url: &str| {
                    tell.send(url.to_string()).unwrap();
                },
            )
        });
        let url = heard.recv_timeout(Duration::from_secs(10)).unwrap();
        let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
        let (port, path) = rest.split_once('/').unwrap();
        let (token, page) = path.split_once('/').unwrap();
        assert_eq!(page, "page/page.html");
        Self {
            dir,
            port: port.parse().unwrap(),
            token: token.to_string(),
            waiting,
        }
    }

    fn host(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// One exchange, as raw bytes: the status and the whole response text.
    fn send(&self, request: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response[9..12].parse().unwrap();
        (status, response)
    }

    fn get(&self, path: &str) -> (u16, String) {
        self.send(&format!(
            "GET {path} HTTP/1.1\r\nHost: {}\r\n\r\n",
            self.host()
        ))
    }

    fn post(&self, body: &str) -> (u16, String) {
        self.send(&format!(
            "POST /{}/answers HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            self.token,
            self.host(),
            self.origin(),
            body.len()
        ))
    }

    fn outcome(self) -> harness_core::Result<Outcome> {
        let outcome = self.waiting.join().unwrap();
        drop(self.dir);
        outcome
    }
}

fn body(response: &str) -> &str {
    response.split_once("\r\n\r\n").unwrap().1
}

#[test]
fn the_page_is_served_with_its_script_and_an_answer_set_ends_the_wait() {
    let served = Served::start(Duration::from_secs(60), None);

    let (status, page) = served.get(&format!("/{}/page/page.html", served.token));
    assert_eq!(status, 200);
    assert!(page.contains("Referrer-Policy: no-referrer"));
    assert!(body(&page).starts_with(PAGE));
    assert!(body(&page).contains(&format!(
        "<script src=\"/{}/ask.js\"></script>",
        served.token
    )));

    let (status, script) = served.get(&format!("/{}/ask.js", served.token));
    assert_eq!(status, 200);
    assert!(script.contains("Content-Type: text/javascript"));
    let setting = body(&script).lines().next().unwrap();
    let setting: serde_json::Value = serde_json::from_str(
        setting
            .strip_prefix("const ASK = ")
            .unwrap()
            .strip_suffix(';')
            .unwrap(),
    )
    .unwrap();
    assert_eq!(setting["endpoint"], format!("/{}/answers", served.token));
    assert_eq!(setting["asks"]["asks"][0]["version"], "v1");
    assert_eq!(setting["words"]["send"], "보내기");
    assert!(setting["deadline"].as_str().unwrap().ends_with('Z'));

    let (status, refused) =
        served.post(r#"{"answers": [{"id": "d-1", "answer": "고칠 곳이 있다"}]}"#);
    assert_eq!(status, 400);
    assert!(body(&refused).contains("적을 것이 있다"), "{refused}");

    let (status, taken) =
        served.post(r#"{"answers": [{"id": "d-1", "answer": "고칠 곳이 있다", "note": "3번"}]}"#);
    assert_eq!(status, 200);
    assert_eq!(body(&taken), r#"{"final":true}"#);

    match served.outcome().unwrap() {
        Outcome::Answered { url, answers } => {
            assert!(url.ends_with("/page/page.html"));
            assert_eq!(answers.len(), 1);
            assert_eq!(answers[0].note.as_deref(), Some("3번"));
            assert_eq!(answers[0].offered.len(), 2);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_request_naming_another_host_is_refused_before_it_is_read() {
    let served = Served::start(Duration::from_secs(60), None);
    let page = format!("/{}/page/page.html", served.token);
    let rebound = format!("127.0.0.1.evil.example:{}", served.port);
    for host in ["evil.example", rebound.as_str(), ""] {
        let (status, _) = served.send(&format!("GET {page} HTTP/1.1\r\nHost: {host}\r\n\r\n"));
        assert_eq!(status, 403, "{host:?}");
    }
    let (status, _) = served.send(&format!("GET {page} HTTP/1.1\r\n\r\n"));
    assert_eq!(status, 403, "no host");
    served.post(r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#);
    served.outcome().unwrap();
}

#[test]
fn a_person_reaching_the_port_through_a_forward_answers_from_there() {
    let served = Served::start(Duration::from_secs(60), None);
    let forwarded = "localhost:8123";
    let (status, _) = served.send(&format!(
        "GET /{}/page/page.html HTTP/1.1\r\nHost: {forwarded}\r\n\r\n",
        served.token
    ));
    assert_eq!(status, 200);
    let body = r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#;
    let post = |origin: &str| {
        served.send(&format!(
            "POST /{}/answers HTTP/1.1\r\nHost: {forwarded}\r\nOrigin: {origin}\r\nContent-Length: {}\r\n\r\n{body}",
            served.token,
            body.len()
        ))
    };
    assert_eq!(
        post(&served.origin()).0,
        403,
        "an origin other than the host's"
    );
    assert_eq!(post(&format!("http://{forwarded}")).0, 200);
    assert!(matches!(
        served.outcome().unwrap(),
        Outcome::Answered { .. }
    ));
}

#[test]
fn an_answer_set_from_another_origin_is_refused() {
    let served = Served::start(Duration::from_secs(60), None);
    let body = r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#;
    for origin in [Some("http://evil.example"), Some("null"), None] {
        let origin = origin
            .map(|o| format!("Origin: {o}\r\n"))
            .unwrap_or_default();
        let (status, _) = served.send(&format!(
            "POST /{}/answers HTTP/1.1\r\nHost: {}\r\n{origin}Content-Length: {}\r\n\r\n{body}",
            served.token,
            served.host(),
            body.len()
        ));
        assert_eq!(status, 403, "{origin:?}");
    }
    assert_eq!(served.post(body).0, 200);
    served.outcome().unwrap();
}

#[test]
fn only_the_token_paths_are_served() {
    let served = Served::start(Duration::from_secs(60), None);
    let t = served.token.clone();
    for path in [
        "/".to_string(),
        format!("/{t}"),
        format!("/{t}/"),
        format!("/{t}/answers"),
        format!("/{t}/page/../decision.md"),
        format!("/{t}/page/%2E%2E/decision.md"),
        format!("/{t}/page/.git/config"),
        format!("/{t}/page/"),
        format!("/{}/page/page.html", "0".repeat(32)),
    ] {
        assert_eq!(served.get(&path).0, 404, "{path}");
    }
    let (status, css) = served.get(&format!("/{t}/page/styles/base.css"));
    assert_eq!(status, 200);
    assert!(css.contains("Content-Type: text/css"));
    assert!(
        !body(&css).contains("ask.js"),
        "only the page takes the script"
    );
    served.post(r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#);
    served.outcome().unwrap();
}

#[test]
fn an_answer_set_past_the_body_limit_is_refused_and_the_wait_goes_on() {
    let served = Served::start(Duration::from_secs(60), None);
    let (status, response) = served.send(&format!(
        "POST /{}/answers HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nContent-Length: {}\r\n\r\n",
        served.token,
        served.host(),
        served.origin(),
        64 * 1024 + 1
    ));
    assert_eq!(status, 413);
    assert!(body(&response).contains("너무 크다"));
    assert_eq!(
        served
            .post(r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#)
            .0,
        200
    );
    assert!(matches!(
        served.outcome().unwrap(),
        Outcome::Answered { .. }
    ));
}

#[test]
fn a_source_changed_while_the_page_was_open_ends_the_wait_as_stale() {
    let served = Served::start(Duration::from_secs(60), None);
    std::fs::write(served.dir.path().join("decision.md"), "# 결정 (고침)\n").unwrap();
    let (status, response) =
        served.post(r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#);
    assert_eq!(status, 409);
    let reply: serde_json::Value = serde_json::from_str(body(&response)).unwrap();
    assert_eq!(reply["final"], true);
    assert!(
        reply["problem"]
            .as_str()
            .unwrap()
            .contains("원천 문서가 바뀌었다")
    );
    assert!(matches!(served.outcome().unwrap(), Outcome::Stale { .. }));
}

#[test]
fn no_answer_in_time_ends_the_wait_unanswered() {
    let served = Served::start(Duration::from_millis(300), None);
    assert!(matches!(
        served.outcome().unwrap(),
        Outcome::Unanswered { .. }
    ));
}

#[test]
fn a_connection_that_sends_nothing_does_not_hold_the_page() {
    let served = Served::start(Duration::from_secs(60), None);
    let _idle = TcpStream::connect(("127.0.0.1", served.port)).unwrap();
    let started = Instant::now();
    assert_eq!(
        served.get(&format!("/{}/page/page.html", served.token)).0,
        200
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    served.post(r#"{"answers": [{"id": "d-1", "answer": "지금 만든다"}]}"#);
    served.outcome().unwrap();
}

struct Refusing;

impl Browser for Refusing {
    fn open(&self, _url: &str) -> Result<(), String> {
        Err("no display".into())
    }
}

#[test]
fn a_page_the_browser_cannot_open_is_not_served() {
    let served = Served::start(Duration::from_secs(60), Some(Box::new(Refusing)));
    let port = served.port;
    let error = served.outcome().unwrap_err();
    assert_eq!(error.code(), ErrorCode::AskBrowserUnopened);
    assert!(
        matches!(error, Error::AskBrowserUnopened { ref reason, .. } if reason == "no display")
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the port is closed"
    );
}
