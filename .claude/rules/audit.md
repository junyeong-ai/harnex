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
  (`audit-ms-timeout`, `audit-mcp-matcher-incomplete`).
- `hook-wiring` — a hook naming a **scaffold artifact** that is absent
  (`audit-hook-script-missing`). Two scopings, each removing a guess. Only the
  `${CLAUDE_PROJECT_DIR}` anchor is read, because it is the one token form that
  denotes a project path by construction. And only manifest destinations are
  judged, because the anchor proves a token is a project path without proving
  the project already built it — `node_modules/.bin/*`, `target/release/*`, and
  a bundler's output are correct wirings that are simply absent on a fresh
  clone. The cost is stated in the module doc: this protects harnex-generated
  wiring, not an operator's own scripts.
  The grammar itself lives in `guard::project_dir` (`.claude/rules/guard.md`),
  read by this auditor and by the scaffold-manifest test alike — a second
  scanner would drift on the first edit to either's shell vocabulary.
- `managed-region` — sentinel-block integrity vs the plugin templates
  declared by the `managed` artifacts of `plugins/harnex/templates/scaffold.toml`
  (`audit-managed-region-edited`, `audit-managed-region-missing`).
- `copy-drift` — a `copy` artifact whose bytes differ from the template that
  emits it, paired by language so a destination is held to the template that
  produced it rather than to any harnex ships (`audit-copy-drift`, Minor). This
  is how a project's own file at a claimed destination becomes visible: the
  scaffold keeps it, and merges the hook fragments that wire into it anyway,
  because ownership is decided per artifact while the wiring lives in another
  one. Advisory rather than gating because three states produce the same bytes —
  the kept incumbent, an edit to harnex's copy, and a harness generated at an
  older plugin version — and the collision rule *instructs* the first, so a
  gating finding would make the manifest contradict itself.
- `fill-marker` — a fill marker the generating step left behind
  (`audit-fill-marker-unresolved`), over `CLAUDE.md` and `.claude/**/*.md`.
  Three reserved grammars now exist — the managed sentinel, the fill marker,
  and the evidence `file:` claim — and each is an exact-match token. Writing one
  literally in prose that a scan reaches makes that file a finding, so an
  example of the syntax goes in a fenced block or an HTML comment, both of which
  the parsers skip, or is paraphrased. The commit that introduced the claim
  grammar tripped over this in its own template.

Spec-vocabulary staleness is deliberately not an audit finding: it describes
this binary's knowledge, not the project under audit, so it rides the
envelope's `warnings[]` on every command (`.claude/rules/spec.md`). As a
finding it would misattribute the problem and make a fixture's zero-findings
assertion fail on a calendar with no code change.

Sentinel parsing routes through `harness_core::sentinel::extract_regions`
— the same util the `spec_facts_sync` drift test uses. Constitution IX:
no parallel sentinel parser.

Managed-region drift reads the `managed` artifacts of
`plugins/harnex/templates/scaffold.toml` — the same manifest the skill emits
from, so a managed pair cannot disagree with the composition it belongs to
(Constitution VII: no project-domain paths in Rust source). Marking a template
managed is a TOML flag, never a code change.

Boundary: audit findings are **deterministic value / structural** checks —
never prose pattern matching, and never an inference about behavior the
settings file does not state. A filename, a script's name, or any other
stand-in for what a verifier's body does is a guess dressed as a check: it
clears the correct spellings it does not recognize and cannot see the
incorrect ones it does. Where an invariant is decidable only at generation,
it is enforced in the template and this auditor stays silent. The cost of an
audit false positive is operator distrust; the benefit is detecting a class
of defects validators do not. Anything short of that ratio belongs in
`validate`, not here. Per `keep-soften-cut`, numeric thresholds (e.g.,
`audit-ms-timeout`) ship as `Minor` advisories — not blocking.
