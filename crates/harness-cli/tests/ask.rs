//! `harnex ask` end to end: the address on stderr, one answer set sent to it,
//! the envelope and exit code that come back, and `current` over the record.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn harnex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harnex"))
}

const ASKS: &str = r#"{
    "sources": ["decision.md"],
    "asks": [
        {"id": "approved:기준", "label": "기준", "version": "v1", "answers": [
            {"name": "승인", "note": "none"}, {"name": "보류", "note": "required"}]}
    ]
}"#;

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("asks.json"), ASKS).unwrap();
    std::fs::write(dir.path().join("decision.md"), "# 결정\n").unwrap();
    std::fs::write(
        dir.path().join("page.html"),
        "<fieldset data-ask=\"approved:기준\" data-version=\"v1\"></fieldset><div data-ask-send></div>",
    )
    .unwrap();
    dir
}

/// A running `ask serve`, at the address it announced on stderr.
struct Served {
    child: Child,
    url: String,
    minutes: String,
    port: u16,
    token: String,
}

impl Served {
    fn start(command: &mut Command) -> Self {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stderr.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let (url, minutes) = line
            .strip_prefix("serving ")
            .and_then(|rest| rest.strip_suffix(" minutes\n"))
            .and_then(|rest| rest.split_once(" for "))
            .unwrap_or_else(|| panic!("the address line: {line:?}"));
        let (port, path) = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|rest| rest.split_once('/'))
            .unwrap();
        Self {
            port: port.parse().unwrap(),
            token: path.split('/').next().unwrap().to_string(),
            url: url.to_string(),
            minutes: minutes.to_string(),
            child,
        }
    }

    fn request(&self, method: &str, path: &str, body: &str) -> String {
        let port = self.port;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(
            stream,
            "{method} /{}/{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
            self.token,
            body.len()
        )
        .unwrap();
        let mut reply = String::new();
        stream.read_to_string(&mut reply).unwrap();
        reply
    }

    fn finish(mut self) -> (i32, serde_json::Value) {
        let mut stdout = String::new();
        self.child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        let status = self.child.wait().unwrap();
        (
            status.code().unwrap(),
            serde_json::from_str(&stdout).unwrap(),
        )
    }
}

/// A test that fails before the command ends must not leave it serving for
/// the minutes it was given.
impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn serve(dir: &Path) -> Command {
    let mut command = harnex();
    command
        .args(["ask", "serve", "page.html", "asks.json"])
        .current_dir(dir);
    command
}

/// Serve, then send `body` once the address is announced, and read what the
/// command printed.
fn serve_and_send(dir: &Path, before: impl FnOnce(), body: &str) -> (i32, serde_json::Value) {
    let served = Served::start(serve(dir).arg("--no-open"));
    before();
    served.request("POST", "answers", body);
    served.finish()
}

#[test]
fn an_answered_ask_prints_its_record_and_current_holds_it() {
    let dir = project();
    let (code, envelope) = serve_and_send(
        dir.path(),
        || {},
        r#"{"answers": [{"id": "approved:기준", "answer": "보류", "note": "3번 기준이 모호하다"}]}"#,
    );
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["ok"], true);
    let data = &envelope["data"];
    assert_eq!(data["outcome"], "answered");
    assert_eq!(data["answers"][0]["note"], "3번 기준이 모호하다");
    assert_eq!(data["answers"][0]["offered"][1]["note"], "required");
    std::fs::write(dir.path().join("answered.json"), data.to_string()).unwrap();

    let current = |dir: &Path| {
        let out = harnex()
            .args(["ask", "current", "answered.json", "asks.json"])
            .current_dir(dir)
            .output()
            .unwrap();
        let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        (out.status.code().unwrap(), envelope)
    };
    let (code, envelope) = current(dir.path());
    assert_eq!(code, 0, "{envelope}");
    assert_eq!(envelope["data"]["items"][0]["state"], "current");

    std::fs::write(
        dir.path().join("asks.json"),
        ASKS.replace("\"v1\"", "\"v2\""),
    )
    .unwrap();
    let (code, envelope) = current(dir.path());
    assert_eq!(code, 1, "{envelope}");
    assert_eq!(envelope["data"]["items"][0]["state"], "changed");
}

#[test]
fn an_ask_whose_source_moved_prints_stale_and_exits_1() {
    let dir = project();
    let source = dir.path().join("decision.md");
    let (code, envelope) = serve_and_send(
        dir.path(),
        || std::fs::write(&source, "# 결정 (고침)\n").unwrap(),
        r#"{"answers": [{"id": "approved:기준", "answer": "승인"}]}"#,
    );
    assert_eq!(code, 1, "{envelope}");
    assert_eq!(envelope["data"]["outcome"], "stale");
}

#[test]
fn an_asks_file_that_breaks_the_schema_is_an_input_error() {
    let dir = project();
    std::fs::write(
        dir.path().join("answered.json"),
        r#"{"outcome": "answered", "url": "u", "answers": [{"id": "approved:기준",
            "version": "v1", "label": "기준", "offered": [{"name": "승인", "note": "none"},
            {"name": "보류", "note": "required"}], "answer": "승인", "note": null,
            "at": "2026-01-01T00:00:00Z"}]}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("asks.json"),
        r#"{"asks": [], "title": "x"}"#,
    )
    .unwrap();
    for args in [
        vec!["ask", "serve", "page.html", "asks.json", "--no-open"],
        vec!["ask", "current", "answered.json", "asks.json"],
    ] {
        let out = harnex()
            .args(&args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(envelope["error"]["code"], "ASK_INPUT_INVALID", "{args:?}");
        let message = envelope["error"]["message"].as_str().unwrap_or_default();
        assert!(message.contains("asks.json"), "{args:?}: {message}");
        let hint = envelope["error"]["hint"].as_str().unwrap_or_default();
        assert!(
            hint.contains("harnex export schema asks"),
            "{args:?}: {hint}"
        );
    }
}

#[test]
fn the_page_closes_the_minutes_given_from_now() {
    let dir = project();
    let served = Served::start(serve(dir.path()).args(["--no-open", "--within", "3"]));
    assert_eq!(served.minutes, "3");
    let script = served.request("GET", "ask.js", "");
    let setting = script
        .split_once("const ASK = ")
        .and_then(|(_, rest)| rest.split_once(";\n"))
        .map(|(json, _)| serde_json::from_str::<serde_json::Value>(json).unwrap())
        .unwrap();
    let deadline: jiff::Timestamp = setting["deadline"].as_str().unwrap().parse().unwrap();
    let off = deadline.duration_since(jiff::Timestamp::now()) - jiff::SignedDuration::from_mins(3);
    assert!(off.abs() < jiff::SignedDuration::from_secs(30), "{off:?}");
    served.request(
        "POST",
        "answers",
        r#"{"answers": [{"id": "approved:기준", "answer": "승인"}]}"#,
    );
    assert_eq!(served.finish().0, 0);
}

/// The platform's opener, found on `PATH` as `open` and `xdg-open`, standing
/// in as a script that records the address it was handed and exits `status`.
#[cfg(unix)]
fn opener(status: u8) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    for name in ["open", "xdg-open"] {
        let path = bin.path().join(name);
        std::fs::write(
            &path,
            format!("#!/bin/sh\nprintf '%s' \"$1\" > \"$OPENED.part\"\nmv \"$OPENED.part\" \"$OPENED\"\nexit {status}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin
}

#[cfg(unix)]
fn with_opener(command: &mut Command, bin: &Path, opened: &Path) {
    let path = std::env::join_paths(std::iter::once(bin.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    command.env("PATH", path).env("OPENED", opened);
}

#[cfg(unix)]
#[test]
fn the_browser_is_handed_the_address_it_announced() {
    let dir = project();
    let bin = opener(0);
    let opened = dir.path().join("opened");
    let mut command = serve(dir.path());
    with_opener(&mut command, bin.path(), &opened);
    let served = Served::start(&mut command);
    let started = Instant::now();
    while !opened.exists() {
        assert!(started.elapsed() < Duration::from_secs(10), "never opened");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read_to_string(&opened).unwrap(), served.url);
    served.request(
        "POST",
        "answers",
        r#"{"answers": [{"id": "approved:기준", "answer": "승인"}]}"#,
    );
    assert_eq!(served.finish().0, 0);

    let bin = opener(3);
    let mut command = serve(dir.path());
    with_opener(&mut command, bin.path(), &opened);
    let out = command.output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "ASK_BROWSER_UNOPENED");
    let hint = envelope["error"]["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("--no-open"), "{hint}");
}
