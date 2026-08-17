---
paths:
  - "crates/harness-core/src/validate/**"
---

# validate — Claude Code surface checks

Six sub-validators. Each reads frontmatter or JSON, returns
[`Finding`] list, never mutates input.

Rule validator — discovery is recursive (`.claude/rules/**/*.md`, per the
memory spec), and every check keys on load scope, which is whether `paths:`
carries at least one glob. A `paths:` key with no value, an empty list, or
blank entries scopes nothing, so the rule loads unconditionally and is judged
as always-loaded — reading key presence alone would exempt it from both checks
below while it costs every turn:
- `paths:` required unless slug in `always_loaded_slugs`.
- `paths:` must be a glob string or a list of glob strings (Major).
- Always-loaded rules: `max_lines` cap (default 200 per Claude Code memory
  spec, which targets unconditionally-loaded context) as Major.
- Path-scoped rules: `max_scoped_lines` cap as Minor, unset by default. The
  always-loaded budget never applies to them — they cost context only on
  their own paths, so a cohesive long rule is not a defect.
- Unparseable frontmatter returns the Blocker alone; load scope is then
  unknown and no budget is asserted.

Skill validator (per <https://code.claude.com/docs/en/skills>):
- `name` ∈ `[a-z0-9-]{1,64}` and equals directory name when declared.
- `description + when_to_use` ≤ `max_description_chars` (1536 cap).
- Body ≤ `max_skill_md_lines` (compaction budget ≈ 5000 tokens).
- `user-invocable` must be boolean if present (Major).
- `context` must be `"fork"` if present (Major).
- `allowed-tools` is a string OR an array of strings — spec accepts both;
  flag only a non-string/non-array value (Major).
- `disallowed-tools` is a string OR an array of strings — same shape
  rules as `allowed-tools` (Major).
- `paths` is a string (comma-separated) OR an array of glob patterns — spec
  accepts both; each glob must compile (Major).
- `hooks` keys validated against `KNOWN_HOOK_EVENTS` (Major).
- `effort` must be one of `low|medium|high|xhigh|max` (Major).
- `agent` / `model` are valid free-form fields — accepted, never flagged
  (a finding for a correct config is CUT-tier noise).
- `reject_unknown_keys` (opt-in, default off): flag any top-level
  frontmatter key outside `KNOWN_SKILL_KEYS` as Major (Claude Code
  silently ignores unknown keys). Skills-only — rule frontmatter is
  intentionally extensible. Extend `KNOWN_SKILL_KEYS` when the spec adds
  a key.
- `flag_side_effect_verbs` (opt-in, default off): match `SIDE_EFFECT_PATTERN`
  against the description; recommend `disable-model-invocation: true` if
  hit. Off by default because the regex matches prose, not intent
  (a read-only skill named `review-commits` should not trip).

Agent validator (per <https://code.claude.com/docs/en/sub-agents>):
- `name` and `description` present; `name` matches `[a-z0-9-]+` (`:` is the
  plugin namespace separator and `agent_type` cannot carry it).
- `permissionMode` ∈ `default|acceptEdits|auto|dontAsk|bypassPermissions|plan|manual`,
  `effort` ∈ `low|medium|high|xhigh|max`, `isolation` ∈ `worktree`,
  `memory` ∈ `user|project|local`, `color` ∈ the eight documented values (Major).
- `maxTurns` is a positive integer; `background` is a boolean.
- `tools` / `disallowedTools` are a string or a list of strings; `skills` is a
  list of strings; `mcpServers` is anything but a scalar (inline definitions
  are mappings).
- `hooks` keys validated against `KNOWN_HOOK_EVENTS` (Major).
- `model` is free-form — aliases and full ids are both valid and the set moves
  with the vendor, the same call the skill validator makes.
- `name` is never checked against the filename: the spec resolves an agent by
  its declared name, unlike a skill whose command comes from its directory.
- No body budget — the body is a system prompt and no documented cap bounds it.
- `reject_unknown_keys` (opt-in, default off): flag any key outside
  `KNOWN_AGENT_KEYS` as Major.

Output-style validator (per <https://code.claude.com/docs/en/output-styles>):
- `keep-coding-instructions` and `force-for-plugin` must be booleans (Major).
  The first defaults to `false`, so a quoted `"true"` silently drops Claude's
  engineering instructions for the whole session with no error anywhere.
- `name` is never required — the spec falls back to the file name.
- No body budget; the body is prompt text.
- `reject_unknown_keys` (opt-in, default off): flag any key outside
  `KNOWN_OUTPUT_STYLE_KEYS`.

Settings validator:
- Every hook event in `hooks` keys must be in `KNOWN_HOOK_EVENTS`
  (sourced from /en/hooks). The set is a permissive superset for typo
  detection — it errs toward accepting, never asserts an exact count.
- `permissions.deny` empty raises a Minor advisory.
- `permissions.defaultMode` must be in `KNOWN_DEFAULT_MODE_VALUES`
  (`default|acceptEdits|plan|auto|dontAsk|bypassPermissions`) if present (Major).
- Project / local scope settings carrying a key in
  `KNOWN_PROJECT_SCOPE_NOOP_KEYS` (the const is the owner — see settings.rs;
  `defaultMode: "auto"` is the value-restricted special case) raise a Major
  advisory — those keys silently no-op outside user/managed.
- `skillOverrides` values must be `on|name-only|user-invocable-only|off` (Major).
- Allow rules whose command base is in `DANGEROUS_ALLOW_BASES`
  (`rm`, `rm -rf`, `curl`, `sudo`) without a deny of the same base raise a
  Minor advisory. Match on the normalized base via `bash_command_base`, which
  collapses the equivalent `cmd:*` / `cmd *` / bare wildcard forms, so both
  spellings are caught and a scoped rule (`rm:./tmp/*`) is not.

Glob-driven validators (rules / skills / agents / output styles) implement
`SurfaceValidator`, which declares the config section that enables them, the
glob they cover, and their slug. `ProjectChecker` drives every one of them
through a single method, so the skipped-vs-ran contract, the `--since` filter,
and the scanned-file count cannot diverge between artifact classes. Adding a
class is an impl plus one call — never a copied method.

When the spec changes, update `KNOWN_HOOK_EVENTS` (or the matching closed
set) and add a test that asserts the new value is accepted.

Commit-msg validator (`[validate.commit_msg]`):
- Each `[[validate.commit_msg.trailers]]` declares `key` plus optional
  `allowed_values` (closed enum) and `required` (presence floor).
- Trailers without `allowed_values` validate by presence-only (any
  non-empty value accepted).
- `required = true` + trailer absent → Blocker finding.
- Trailer values outside `allowed_values` → Major finding.
- Indented lines are body prose, not trailers (per git convention).

When adding a new trailer enum, extend the `[[validate.commit_msg.trailers]]`
config block and add a test asserting both the accept and reject paths.
