---
paths:
  - "crates/harness-core/src/codegen/**"
---

# codegen — sentinel-block sync

Source is a TOML, JSON, or YAML file with a string array at a dot-path,
selected per group via `source_format` (default `toml`). Targets are
files with `BEGIN <slug>` / `END <slug>` sentinel lines. Renderers replace
content strictly between sentinels.

When adding a new source format:
1. Add a `SourceFormat` variant (single source of truth — its
   `from_str`/`as_str`/`ALL` are the strings both `Config::validate`
   and `SentinelSyncer::run` consume).
2. Add a match arm in `source::load_source` parsing into
   `serde_json::Value` — exhaustive match on the enum enforces this at
   compile time.
3. Add a `load_source` unit test in `codegen::source::tests` for the
   new format.

When adding a new renderer:
1. Add a `RendererStrategy` variant (single source of truth — its
   `from_str`/`as_str`/`ALL` are the strings both `Config::validate`
   and the factory consume).
2. Implement [`Renderer`] for the new struct in `codegen/renderer.rs`.
3. Add a match arm in `renderer_for` — exhaustive match on the enum
   enforces this step at compile time.
4. Add at least one unit test in `codegen::renderer::tests`.

Self-consistency: `Config::validate` calls `RendererStrategy::from_str`
to reject unknown format strings and rejects target files that are also
sources (cycle).

Idempotence: a `sync` whose source already equals the current target
content writes nothing (`SyncOutcome.changed == false`).
