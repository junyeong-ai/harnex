---
paths:
  - "crates/harness-core/src/validate/**"
---

# validate — Claude Code surface checks

Three sub-validators. Each reads frontmatter or JSON, returns
[`Finding`] list, never mutates input.

Rule validator:
- `max_lines` cap (default 200 per Claude Code memory spec).
- `paths:` frontmatter required unless slug in `always_loaded_slugs`.

Skill validator (per <https://code.claude.com/docs/en/skills>):
- `name` ∈ `[a-z0-9-]{1,64}` and equals directory name when declared.
- `description + when_to_use` ≤ `max_description_chars` (1536 cap).
- Body ≤ `max_skill_md_lines` (compaction budget ≈ 5000 tokens).
- Side-effect verbs in description without `disable-model-invocation: true`
  raise a Minor finding per spec recommendation.
- `user-invocable` must be boolean if present (Major).
- `context` must be `"fork"` if present (Major).
- `allowed-tools` must be array of strings (Major).
- `paths` must be array of valid glob patterns (Major).
- `hooks` keys validated against `KNOWN_HOOK_EVENTS` (Major).
- `effort` must be one of `low|medium|high|xhigh|max` (Major).
- `agent` / `model` are valid free-form fields — accepted, never flagged
  (a finding for a correct config is CUT-tier noise).
- `reject_unknown_keys` (opt-in, default off): flag any top-level
  frontmatter key outside `KNOWN_SKILL_KEYS` as Major (Claude Code
  silently ignores unknown keys). Skills-only — rule frontmatter is
  intentionally extensible. Extend `KNOWN_SKILL_KEYS` when the spec adds
  a key.

Settings validator:
- Every hook event in `hooks` keys must be in `KNOWN_HOOK_EVENTS`
  (sourced from /en/hooks). The set is a permissive superset for typo
  detection — it errs toward accepting, never asserts an exact count.
- `permissions.deny` empty raises a Minor advisory.
- `skillOverrides` values must be `on|name-only|user-invocable-only|off` (Major).
- Overly permissive allow patterns (`rm:*`, `curl:*`, `sudo:*`, `rm -rf:*`)
  without corresponding deny raise a Minor advisory.

When the spec changes, update `KNOWN_HOOK_EVENTS` and add a test that
asserts the new event is accepted.

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
