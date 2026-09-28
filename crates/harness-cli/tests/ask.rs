//! `harnex ask` end to end: the address on stderr, one answer set sent to it,
//! the envelope and exit code that come back, and `current` over the record.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

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

/// Serve, then send `body` once the address is announced, and read what the
/// command printed.
fn serve_and_send(
    dir: &std::path::Path,
    before: impl FnOnce(),
    body: &str,
) -> (i32, serde_json::Value) {
    let mut child = harnex()
        .args(["ask", "serve", "page.html", "asks.json", "--no-open"])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line
        .strip_prefix("serving ")
        .and_then(|rest| rest.strip_suffix(" for 120 minutes\n"))
        .unwrap_or_else(|| panic!("the address line: {line:?}"));
    let rest = url.strip_prefix("http://127.0.0.1:").unwrap();
    let (port, path) = rest.split_once('/').unwrap();
    let token = path.split('/').next().unwrap();

    before();
    let mut stream = TcpStream::connect(("127.0.0.1", port.parse::<u16>().unwrap())).unwrap();
    write!(
        stream,
        "POST /{token}/answers HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();

    let out = child.wait_with_output().unwrap();
    let envelope = serde_json::from_slice(&out.stdout).unwrap();
    (out.status.code().unwrap(), envelope)
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

    let current = |dir: &std::path::Path| {
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
        dir.path().join("asks.json"),
        r#"{"asks": [], "title": "x"}"#,
    )
    .unwrap();
    for args in [
        vec!["ask", "serve", "page.html", "asks.json", "--no-open"],
        vec!["ask", "current", "asks.json", "asks.json"],
    ] {
        let out = harnex()
            .args(&args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let envelope: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(envelope["error"]["code"], "ASK_INPUT_INVALID", "{args:?}");
    }
}
