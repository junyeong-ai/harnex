---
paths:
  - "crates/harness-core/src/audit/**"
---

# audit — harness-engineering compliance gate

`ProjectAuditor::run` is the single entry point. Findings aggregate under
one envelope with the same deterministic sort order as `check` (severity
ascending, slug, path).

Sub-auditors dispatch through [`AuditCheckKind`] — a closed-set
discriminator enum (single source of truth). `ProjectAuditor::run` iterates
`AuditCheckKind::ALL` and matches every variant **exhaustively**. Adding a
new sub-auditor:
1. Add an `AuditCheckKind` variant + its `from_str` / `as_str` arms.
2. Add a match arm in `ProjectAuditor::run` — the compiler enforces
   exhaustiveness, so missing this step is a build error.
3. Implement the sub-auditor in `audit/<name>.rs` (visibility `pub(crate)`
   — only `ProjectAuditor` exposes a public entry).
4. Add `from_str_round_trips_every_variant` covers the new variant
   automatically; add slug-specific behavior tests under the sub-auditor's
   `#[cfg(test)] mod tests`.

Sub-auditor slugs (current):
- `settings-drift` — `.claude/settings.json` value compliance
  (`audit-ms-timeout`, `audit-mcp-matcher-incomplete`,
  `audit-stop-blocking-suspect`).
- `managed-region` — sentinel-block integrity vs the plugin templates
  declared in `plugins/harnex/templates/managed-files.toml`
  (`audit-managed-region-edited`, `audit-managed-region-missing`).

Sentinel parsing routes through `harness_core::sentinel::extract_regions`
— the same util the `spec_facts_sync` drift test uses. Constitution IX:
no parallel sentinel parser.

Managed-region drift loads `plugins/harnex/templates/managed-files.toml`
as the file-pair manifest — Constitution VII: no project-domain paths in
Rust source. Adding a new managed-region template is a TOML entry, never a
code change.

Boundary: audit findings are **deterministic value / structural** checks —
never prose pattern matching. The cost of an audit false positive is
operator distrust; the benefit is detecting a class of defects validators
do not. Anything short of that ratio belongs in `validate`, not here. Per
`keep-soften-cut`, numeric thresholds (e.g., `audit-ms-timeout`) ship as
`Minor` advisories — not blocking.
