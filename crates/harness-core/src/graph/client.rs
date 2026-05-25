//! Subprocess-based nodex client behind a runner trait.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};

use super::node::{GraphDiff, NodeRef};

/// Abstracted nodex invocation. The trait exists for **two specific reasons**
/// (NOT speculative future flexibility):
///
/// 1. **External process boundary** — spawning `nodex` is one of the few
///    places the toolkit shells out. Wrapping it in a trait centralises
///    that boundary and prevents leaking `std::process::Command` into
///    consumer code.
/// 2. **Test seam** — `NodexClient<MockRunner>` in tests substitutes
///    canned envelope responses, so parsing logic is verified without
///    requiring `nodex` on PATH in CI.
///
/// New nodex-shell invocations should be added as methods on
/// [`NodexClient`] (using `self.runner.run(&[...])`), not as parallel
/// runners. Adding a second runner impl beyond `DefaultNodexRunner` +
/// the test mock is YAGNI — push back on it.
pub trait NodexRunner: Send + Sync {
    /// Run `nodex` with `args`. Return raw stdout bytes (expected to be a
    /// single JSON envelope per the nodex contract).
    fn run(&self, args: &[&str]) -> Result<String>;
}

/// Spawns a real `nodex` binary under a working directory.
pub struct DefaultNodexRunner {
    binary: PathBuf,
    working_dir: PathBuf,
}

impl DefaultNodexRunner {
    pub fn new(binary: impl Into<PathBuf>, working_dir: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            working_dir: working_dir.into(),
        }
    }

    /// Find `nodex` on PATH. Returns None if the binary is missing.
    pub fn detect(working_dir: impl Into<PathBuf>) -> Option<Self> {
        let probe = Command::new("nodex").arg("--version").output().ok()?;
        if !probe.status.success() {
            return None;
        }
        Some(Self {
            binary: PathBuf::from("nodex"),
            working_dir: working_dir.into(),
        })
    }
}

impl NodexRunner for DefaultNodexRunner {
    fn run(&self, args: &[&str]) -> Result<String> {
        let output = Command::new(&self.binary)
            .args(args)
            .current_dir(&self.working_dir)
            .output()
            .map_err(|e| Error::GraphSpawnFailure {
                message: format!("spawn nodex: {e}"),
            })?;
        // nodex itself uses exit 1 for validation findings — we still parse stdout.
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// Typed query client over [`NodexRunner`].
pub struct NodexClient<R: NodexRunner = DefaultNodexRunner> {
    runner: R,
}

impl<R: NodexRunner> NodexClient<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn version(&self) -> Result<String> {
        let raw = self.runner.run(&["--version"])?;
        Ok(raw.trim().to_string())
    }

    pub fn backlinks(&self, node_id: &str) -> Result<Vec<NodeRef>> {
        let raw = self.runner.run(&["query", "backlinks", node_id])?;
        parse_items(&raw)
    }

    pub fn orphans(&self) -> Result<Vec<NodeRef>> {
        let raw = self.runner.run(&["query", "orphans"])?;
        parse_items(&raw)
    }

    pub fn stale(&self) -> Result<Vec<NodeRef>> {
        let raw = self.runner.run(&["query", "stale"])?;
        parse_items(&raw)
    }

    pub fn nodes_of_kind(&self, kind: &str) -> Result<Vec<NodeRef>> {
        let raw = self.runner.run(&["query", "nodes", "--kind", kind])?;
        parse_items(&raw)
    }

    /// Compute the structural delta between two git refs. Wraps
    /// `nodex diff <ref-a> <ref-b>`. The returned [`GraphDiff`] is loose:
    /// callers reach into `extra` for unfamiliar fields.
    pub fn diff(&self, ref_a: &str, ref_b: &str) -> Result<GraphDiff> {
        let raw = self.runner.run(&["diff", ref_a, ref_b])?;
        parse_diff(&raw)
    }
}

impl NodexClient<DefaultNodexRunner> {
    /// Construct a client with the default runner anchored at `working_dir`.
    pub fn anchored(working_dir: impl AsRef<Path>) -> Option<Self> {
        DefaultNodexRunner::detect(working_dir.as_ref().to_path_buf()).map(NodexClient::new)
    }
}

#[derive(Debug, Deserialize)]
struct EnvelopeProbe {
    ok: bool,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    error: Value,
}

#[derive(Debug, Deserialize)]
struct ItemsPayload {
    // `items` is REQUIRED (no serde default): a success envelope whose
    // object `data` lacks `items` is malformed and must surface as
    // GraphResponseInvalid, not silently parse as zero results — which would
    // hide a graph failure as "0 backlinks/orphans" and corrupt retirement
    // decisions. The direct-array compat path wraps as {items: [...]}, so it
    // always satisfies this.
    items: Vec<NodeRef>,
}

/// Parse a nodex success envelope, expecting `data` shaped like [`GraphDiff`].
fn parse_diff(raw: &str) -> Result<GraphDiff> {
    let env: EnvelopeProbe =
        serde_json::from_str(raw).map_err(|e| Error::GraphResponseInvalid {
            message: format!("nodex envelope parse: {e}"),
        })?;
    if !env.ok {
        return Err(Error::GraphSpawnFailure {
            message: format!("nodex returned error: {}", env.error),
        });
    }
    serde_json::from_value(env.data).map_err(|e| Error::GraphResponseInvalid {
        message: format!("nodex diff payload parse: {e}"),
    })
}

/// Parse a nodex success envelope, expecting `data.items: NodeRef[]`.
fn parse_items(raw: &str) -> Result<Vec<NodeRef>> {
    let env: EnvelopeProbe =
        serde_json::from_str(raw).map_err(|e| Error::GraphResponseInvalid {
            message: format!("nodex envelope parse: {e}"),
        })?;
    if !env.ok {
        return Err(Error::GraphSpawnFailure {
            message: format!("nodex returned error: {}", env.error),
        });
    }
    // data may be {items: [...]} or directly an array; handle both.
    let items_value: Value = if env.data.is_array() {
        serde_json::json!({ "items": env.data })
    } else {
        env.data
    };
    let payload: ItemsPayload =
        serde_json::from_value(items_value).map_err(|e| Error::GraphResponseInvalid {
            message: format!("nodex data.items parse: {e}"),
        })?;
    Ok(payload.items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct MockRunner {
        responses: Mutex<Vec<String>>,
        calls: Mutex<Vec<Vec<String>>>,
    }

    impl MockRunner {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().map(String::from).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl NodexRunner for MockRunner {
        fn run(&self, args: &[&str]) -> Result<String> {
            self.calls
                .lock()
                .unwrap()
                .push(args.iter().map(|s| s.to_string()).collect());
            let mut resp = self.responses.lock().unwrap();
            if resp.is_empty() {
                Ok(r#"{"ok":true,"data":{"items":[]}}"#.into())
            } else {
                Ok(resp.remove(0))
            }
        }
    }

    #[test]
    fn version_strips_whitespace() {
        let runner = MockRunner::new(vec!["nodex 0.10.0\n"]);
        let client = NodexClient::new(runner);
        assert_eq!(client.version().unwrap(), "nodex 0.10.0");
    }

    #[test]
    fn backlinks_passes_node_id() {
        let runner = MockRunner::new(vec![
            r#"{"ok":true,"data":{"items":[{"id":"adr-x","kind":"adr"}]}}"#,
        ]);
        let client = NodexClient::new(runner);
        let nodes = client.backlinks("adr-x").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "adr-x");
        assert_eq!(nodes[0].kind.as_deref(), Some("adr"));
    }

    #[test]
    fn supports_array_shaped_data() {
        let runner = MockRunner::new(vec![r#"{"ok":true,"data":[{"id":"x"},{"id":"y"}]}"#]);
        let client = NodexClient::new(runner);
        let nodes = client.orphans().unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn surfaces_error_envelope() {
        let runner = MockRunner::new(vec![r#"{"ok":false,"error":{"code":"X","message":"y"}}"#]);
        let client = NodexClient::new(runner);
        let err = client.stale().unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::GraphSpawnFailure);
    }

    #[test]
    fn object_data_without_items_is_invalid_not_empty() {
        // A success envelope whose object `data` lacks `items` is malformed —
        // it must surface GraphResponseInvalid, never parse as zero results
        // (which would hide a graph failure as "0 backlinks").
        let runner = MockRunner::new(vec![r#"{"ok":true,"data":{"count":0}}"#]);
        let client = NodexClient::new(runner);
        let err = client.backlinks("x").unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::GraphResponseInvalid);
    }

    #[test]
    fn diff_parses_added_and_removed() {
        let runner = MockRunner::new(vec![
            r#"{"ok":true,"data":{
                "added_nodes":[{"id":"new-1","kind":"adr"}],
                "removed_nodes":[{"id":"old-1","kind":"adr"}],
                "status_transitions":[{"id":"x","from":"active","to":"superseded"}],
                "field_changes":[],
                "added_edges":[],
                "removed_edges":[]
            }}"#,
        ]);
        let client = NodexClient::new(runner);
        let d = client.diff("HEAD~1", "HEAD").unwrap();
        assert_eq!(d.added_nodes.len(), 1);
        assert_eq!(d.removed_nodes.len(), 1);
        assert_eq!(d.status_transitions.len(), 1);
    }

    #[test]
    fn diff_tolerates_minimal_payload() {
        let runner = MockRunner::new(vec![r#"{"ok":true,"data":{}}"#]);
        let client = NodexClient::new(runner);
        let d = client.diff("a", "b").unwrap();
        assert!(d.added_nodes.is_empty());
        assert!(d.removed_nodes.is_empty());
    }

    #[test]
    fn extra_fields_preserved_in_node_ref() {
        let runner = MockRunner::new(vec![
            r#"{"ok":true,"data":{"items":[{"id":"x","kind":"adr","supersedes":"y","custom":42}]}}"#,
        ]);
        let client = NodexClient::new(runner);
        let nodes = client.nodes_of_kind("adr").unwrap();
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].extra.contains_key("supersedes"));
        assert!(nodes[0].extra.contains_key("custom"));
    }
}
