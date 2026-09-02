//! The committed schema is the type's.
//!
//! `schemas/harness.schema.json` is what an editor and a generated harness
//! read `harness.toml` against, and it is emitted from `Config` — so a doc
//! comment or a field that moves without it leaves the reader holding a
//! contract the binary no longer keeps. CI compares the two; this holds the
//! same comparison where a change is made, before it is pushed.

use std::path::PathBuf;

use harness_core::export::{SchemaTarget, schema_for};

#[test]
fn the_committed_config_schema_is_what_the_binary_emits() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/harness.schema.json");
    let committed = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    // The same bytes `harnex export schema config --raw` writes: pretty JSON
    // and a trailing newline.
    let emitted = format!(
        "{}\n",
        serde_json::to_string_pretty(&schema_for(SchemaTarget::Config)).expect("serialises")
    );
    assert!(
        committed == emitted,
        "schemas/harness.schema.json is not what `Config` emits; regenerate it with \
         `harnex export schema config --raw > schemas/harness.schema.json`"
    );
}
