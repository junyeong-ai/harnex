# harnex

Harness engineering for Claude Code projects. harnex has two surfaces:

- **The harnex plugin** (primary) — a Claude Code skill that *generates*
  project-fit, project-native harness tooling (hooks, `settings.json`,
  `CLAUDE.md`, path-scoped rules) into a target repo, in that repo's own
  language, from verified spec-correct templates. The value is the
  knowledge of getting the Claude Code spec right, distributed as a skill —
  not a runtime you depend on.
- **The `harness` binary** (oracle) — a Pure-Rust, JSON-first CLI that
  deterministically verifies a harness: provenance, closed-schema telemetry,
  lifecycle, runtime guards, a unified validation gate. It is the
  spec-correct reference the plugin's templates are checked against.

## Why

Modern Claude follows in-context conventions well. What it cannot do alone
is keep its harness spec-correct as the upstream surface evolves, enforce
what the runtime would silently corrupt, or fit one harness to many
languages and module shapes. harnex centralizes the *correctness knowledge*
and emits a harness each project owns — never a shared binary every project
must couple to.

## The plugin

A single-skill plugin under `plugins/harnex/`, distributed by the marketplace
at `.claude-plugin/marketplace.json`. Install, then drive it by mode:

```
/plugin marketplace add junyeong-ai/harnex
/plugin install harnex@harnex

/harnex scaffold      # greenfield: compose a full harness from templates
/harnex extend        # brownfield: add one guardrail in the incumbent idiom
/harnex audit         # read-only: gap report (drift, over-constraint, prose-only musts)
/harnex regenerate    # re-derive against the current Claude Code spec
```

It detects the stack from lockfile + manifest (TypeScript/pnpm,
Python/uv, Rust/cargo), composes the safety-critical pieces from `templates/`, and
never free-generates a hook or permission rule. Knowledge lives in
`reference/` (the spec facts, the enforced-vs-advisory split, the
keep/soften/cut principle, the language matrix, the exploration procedure).

## The oracle binary

```bash
cargo build --release          # → ./target/release/harness
```

Requires Rust 1.95+.

## IDE integration

`schemas/harness.schema.json` ships in this repo. Point your TOML
language server at it for autocomplete + validation on `harness.toml`:

- **Taplo / VS Code Even-Better-TOML**: the generated `harness.toml`
  includes a `#:schema <url>` directive at the top — replace
  `<owner>/<repo>` with your fork's path, or use a `file://` URL of
  `schemas/harness.schema.json` in your local checkout.
- **IntelliJ family**: Languages & Frameworks → Schemas and DTDs → JSON
  Schema Mappings → add `harness.schema.json` for the pattern `harness.toml`.

Regenerate after upstream schema changes:

```bash
harness export schema config --raw > schemas/harness.schema.json
```

(`--raw` emits the bare schema; without it the schema is wrapped in the
standard JSON envelope for programmatic consumers.)

## Oracle quickstart

Scaffolding a fresh harness is the plugin's job (`/harnex scaffold`). The
binary verifies one once it exists:

```bash
cd your-project/

# Start from an example config (or let /harnex scaffold generate it)
cp <harnex>/examples/harness.toml.minimal harness.toml

# Unified gate — every enabled validator in one JSON envelope
./harness check
./harness check --fix      # auto-fix what can be fixed (currently: codegen sync)
```

`examples/harness.toml.minimal` enables just evidence (provenance verifier)
and telemetry (event ledger) — the smallest useful surface.
`examples/harness.toml.team` is the full-surface config (adds
validate.rules/skills, policy.permissions, lifecycle, codegen, …). Start
from one and extend with `[[kinds]]`, `[[lifecycle.consumer_detectors]]`,
`[[codegen.groups]]`, `[[policy.versions]]`, `[validate.commit_msg]` as your
project grows.

## Command surface

```
harness check [--since <ref>] [--fix]                  # unified validation gate
harness audit [--plugin-root <path>]                   # spec drift + managed-region integrity

harness evidence verify <files...>
harness telemetry append --kind K --payload <json>
harness telemetry count --kind K [--since <rfc3339>]
harness telemetry report [--kind K] [--window 1,7,30,90]

harness codegen sync | check

harness policy permissions generate | audit [--path <p>]
harness policy versions show | check --tool T --installed V

harness validate rules <files...>
harness validate skills <files...>
harness validate settings [<path>]
harness validate commit-msg <path>                     # closed-enum trailer

harness lifecycle observe --tag T --text X --source S
harness lifecycle candidates
harness lifecycle promote --tag T --text X --decision-text "..."
harness lifecycle reject  --tag T --text X --decision-text "..."
harness lifecycle defer   --tag T --text X --decision-text "..."
harness lifecycle demote  --tag T --text X --decision-text "..."
harness lifecycle classify --kind K --path P [--silent]
harness lifecycle retire [--window N]
harness lifecycle decisions [--tag T] [--decision D]

harness guard hook-event                               # parse stdin hook JSON
harness guard hook-run <prog> [args...]                # standard hook wrapper
harness guard hook-stop <prog> [args...]               # Stop hook (always exit 0)
harness guard stop-audit [--session ID]                # fresh-context Stop audit

harness graph version | backlinks <id> | orphans | stale | nodes --kind K | diff <a> <b>

harness export schema {config|envelope|finding|event|permissions|error-codes|all}

harness completions <bash|zsh|fish|powershell|elvish> [--raw]
```

By default every command emits one JSON envelope on stdout; the explicit raw
modes (`export schema --raw`, `completions --raw`) emit the bare artifact for
committing to disk. Exit code: 0 = success, 1 = gating finding (blocker or
major), 2 = runtime failure.

## What the oracle covers

The `harness` binary covers the universal Claude Code harness patterns;
the plugin generates the project-native wiring that uses them. Universal
patterns covered out of the box:

- Provenance verification on docs
- Append-only telemetry with a closed payload schema
- Sentinel-block enum codegen across many files
- Permission profiles (`baseline`, `git-strict`, `gcp-strict`, `aws-strict`,
  and per-language `*-dev`) for Claude Code settings
- Claude Code spec compliance (rules / skills / settings frontmatter)
- Promotion + retirement lifecycle for learnings
- Settings.json hook adapter (the documented hook events)
- Single-command CI gate

Project-specific lint (language ASTs, internal data models, design systems,
package allowlists, multi-phase spec orchestrators) is intentionally out of
scope — that belongs with the project's domain knowledge, not with harnex.

## Enterprise adoption

Organizations rolling harnex out across many repositories drive the plugin
through Claude Code's managed-settings surface so floors are set centrally
and individual repos cannot weaken them. The integration points:

- **Pin the marketplace.** Deploy a `managed-settings.json` with
  `strictKnownMarketplaces` set to `[{"source": "github", "repo":
  "junyeong-ai/harnex"}]` (or your fork). Combined with
  `blockedMarketplaces`, this prevents adoption of unreviewed plugins
  while still allowing harnex.
- **Pin enforced floors.** Set `permissions.allowManagedPermissionRulesOnly: true`
  in managed settings so ONLY managed-scope permission rules are honored —
  user / project / local permission rules are then ignored, not merged. To make
  the `baseline` deny a non-removable floor under this policy, DEPLOY that deny
  set in the managed settings itself; a deny shipped only in a project's
  `permissions.deny.json` would be ignored. (Without this policy, rules from all
  scopes merge and the project deny applies.)
- **Pin behavioral guidance.** The managed `claudeMd` key carries the
  organization-wide instructions delivered before any project CLAUDE.md
  ("Always run `make lint` before committing", compliance reminders).
  This survives `claudeMdExcludes` at every other scope.
- **Optional hard-lock plugin surface.** Set
  `strictPluginOnlyCustomization: ["skills", "hooks"]` to require that
  every skill or hook be plugin-managed (not freely added at user /
  project scope). harnex stays usable because its content ships as a
  plugin; everything else routes through the marketplace.
- **Disable skill shell injection.** Set `disableSkillShellExecution:
  true` in managed settings to neutralise `` !`<command>` `` substitution
  in user / project / plugin / additional-directory skills (bundled and
  managed skills are exempt). harnex's templates do not rely on
  shell-injection, so it remains fully functional under this policy.

See `https://code.claude.com/docs/en/settings` for the complete managed
settings surface and OS-specific deployment paths (`managed-settings.d/`,
plist, registry, Group Policy).

## Operating context

Day-to-day operation is delegated to Claude Code. See `CLAUDE.md` and
`.claude/rules/` for the AI operating context. This README is the only
file written for humans; everything else under this repo is consumed
directly by Claude.

## License

MIT.
