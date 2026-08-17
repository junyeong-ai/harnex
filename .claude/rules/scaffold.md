# scaffold — the composition manifest

`plugins/harnex/templates/scaffold.toml` declares every template-derived
artifact a generated harness contains. Emissions whose content comes from the
project rather than from a template — `rustfmt.toml`'s detected edition, a
cloud permission profile — belong to the skill, which names them beside its
reference to this file. `ScaffoldManifest::load` parses and validates it; nothing in
Rust names a template or destination path (Constitution VII).

Three consumers, one declaration:
- the skill emits the artifacts,
- `tests/plugin_scaffold_validates.rs` builds its fixture from them,
- `ProjectAuditor` reports coverage and drives the managed-region auditor.

A file list written a second time is the one that drifts — the fixture omitted
every hook while reporting a clean scaffold before this manifest existed.

## Tiers

`Tier::Foundation` carries no language dependency and is what a stack with no
profile still receives. `Tier::Language` needs a detected stack. The split is
what makes a partial harness expressible instead of an all-or-nothing refusal.

Load-time validation rejects: a destination that is absolute, `~`-rooted, or
escapes with `..`; a template escaping the templates directory; `{lang}` in a
foundation artifact (the tier must resolve with no language); `merge` into a
non-JSON destination; an artifact that is both merged and managed.

## Adding an artifact

1. Add the `[[artifact]]` block. `merge` = contributes to a JSON key path;
   absent = copied verbatim. `executable` = written 0o755. `managed` = carries
   sentinels, so the managed-region auditor compares it to its template.
2. Nothing else changes for a per-language artifact: `{lang}` resolves against
   the `<lang>-dev` members of `PermissionProfile::ALL`.
3. `tests/scaffold_manifest.rs` holds the relations both ways — every named
   template exists for every language, every per-language template is claimed,
   every non-merge destination is claimed once, and the foundation tier wires
   only foundation artifacts.
