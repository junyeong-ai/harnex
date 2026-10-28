---
paths:
  - "crates/harness-core/src/validate/**"
governs:
  concept: the Claude Code surface validators
  live_truth:
    - crates/harness-core/src/validate
---

# validate — Claude Code surface checks

Seven sub-validators. Each reads frontmatter or JSON, returns
[`Finding`] list, never mutates input.

`validate::path_globs` owns the `paths:` grammar shared by rule and skill
validation, `context resolve` and check's `paths-reading` arm; its module doc
states what `DeclaredPaths::read` refuses and where the two `Reading`s part. A
refusal is a `Defect` naming a spelling every reader takes alike, never a match
as one release of the matcher would make it. Each `DefectKind` stands on one of
four grounds, and a new one takes the same: Claude Code splitting, expanding or
trimming an entry otherwise than its syntax says, as its pipeline decides it; a
measured disagreement between `ignore` 7.0.5, 7.0.14 and git; a spelling that
matches nothing it says; or an escape or bracket expression the module doc
names. Draw those two by the grammar that reads them, never by measured
contexts, because the next context breaks a line drawn by cases: escapes by
class, since the regex grammar gives meaning only to an escaped letter or digit
and the bundled release rewrites a few characters before it; brackets as a
closed set, since a member's reading turns on its neighbours. A glob judged
only as written misses what the matcher is given once the trailing `/**` is
dropped, and one judged alone misses the `!` globs around it.

Rule validator — discovery is recursive (`.claude/rules/**/*.md`, per the
memory spec), and every check keys on load scope, which is whether `paths:`
carries a glob other than `**` once a trailing `/**` is removed —
`path_globs::declares_scope` states the rule the runtime applies. A `paths:`
key with no value, an empty list, blank entries, or only `**` scopes nothing,
so the rule loads unconditionally and is judged as always-loaded — reading key
presence alone would exempt it from both checks below while it costs every
turn:
- `paths:` required unless slug in `always_loaded_slugs`.
- `paths:` must be a glob string or a list of glob strings, each inside the
  grammar (Major, one finding per `Defect`).
- Always-loaded rules: `max_lines` cap (default 200 per Claude Code memory
  spec, which targets unconditionally-loaded context) as Major.
- Path-scoped rules: `max_scoped_lines` cap as Minor, unset by default. The
  always-loaded budget never applies to them — a rule's own text costs
  context only on its paths, so a cohesive long rule is not a defect.
- Unparseable frontmatter returns the Blocker alone and asserts no budget:
  the runtime loads such a rule under whatever keys its repair recovers,
  which is not the scope the author wrote.
- `governs:` shape is checked wherever declared (`harness_core::governs` owns
  the grammar); `require_governs` (opt-in) demands one on path-scoped rules
  only — always-loaded rules cross no load boundary and may have no truth in
  the tree, and a malformed declaration counts as declared so one defect
  yields one finding. Existence of the declared truths belongs to `check`'s
  `governs` arm, which has the tree.

Skill validator (per <https://code.claude.com/docs/en/skills>):
- `name` may differ from the directory. Reject context-independent invalid
  names: length, controls, blank-like characters, angle brackets and spacing.
  Leave contextual emoji and script shaping to the runtime.
- Validate directory identity independently of the display alias for project
  and user skills. `SkillScope::Plugin` leaves folder sanitization to the
  runtime; callers declare scope rather than inferring it from a path.
- The listing text ≤ `max_description_chars`. `always_loaded` owns both the
  text (`skill_listing_text`, the runtime's fallback to the body included)
  and how it is counted (`runtime_len`, UTF-16 code units); the default is
  the runtime's own cut (`LISTING_ENTRY_CAP`), which a project's
  `skillListingMaxDescChars` moves and this default does not follow.
- File ≤ `max_skill_md_lines`; line limits do not measure compaction tokens.
- `disable-model-invocation`, `user-invocable` and `background` are flags as
  `frontmatter::flag` reads them — the runtime's spellings, `yes`/`on`/`1`
  included (Major). The always-loaded listing reads the same owner, so the two
  cannot disagree about whether a skill is hidden.
- `context` must be `"fork"` if present (Major).
- `allowed-tools` is a string OR an array of strings — spec accepts both;
  flag only a non-string/non-array value (Major).
- `disallowed-tools` is a string OR an array of strings — same shape
  rules as `allowed-tools` (Major).
- `paths` is a string (comma-separated) OR an array of glob patterns — spec
  accepts both; each glob inside the grammar, as for a rule (Major).
- `hooks` keys validated against `KNOWN_HOOK_EVENTS` (Major).
- `effort` must be in `KNOWN_EFFORT_LEVELS` (Major).
- `agent` / `model` are valid free-form fields — accepted, never flagged
  (a finding for a correct config is CUT-tier noise).
- `reject_unknown_keys` (opt-in, default off): flag any top-level
  frontmatter key outside `KNOWN_SKILL_KEYS` as Major (Claude Code
  silently ignores unknown keys). Rule frontmatter is intentionally
  extensible. Extend `KNOWN_SKILL_KEYS` when the spec adds
  a key.
- `flag_side_effect_verbs` (opt-in, default off): match `SIDE_EFFECT_PATTERN`
  against the description; recommend `disable-model-invocation: true` if
  hit. Off by default because the regex matches prose, not intent
  (a read-only skill named `review-commits` should not trip).

Agent validator (per <https://code.claude.com/docs/en/sub-agents>):
- `name` and `description` present; `agents::name_is_valid` owns the name
  contract shared with the always-loaded listing.
- `permissionMode` ∈ `KNOWN_PERMISSION_MODES`, `effort` ∈
  `KNOWN_EFFORT_LEVELS`, `isolation` ∈ `KNOWN_ISOLATION_MODES`, `memory` ∈
  `KNOWN_MEMORY_SCOPES`, `color` ∈ `KNOWN_COLORS` (Major).
- `maxTurns` is a positive integer; `background` is a boolean.
- `tools` / `disallowedTools` are a string or a list of strings; `skills` is a
  list of strings. Whether each named project skill can be preloaded is
  `audit_preload`'s, which `check` runs as its own arm because it reads the
  skill too; a name with no project skill beside the agent — user, plugin,
  nested — is not judged; `mcpServers` is neither a scalar nor a list carrying an
  element that names nothing (a bool, a number, a null). A mapping element
  stays accepted — an inline definition is one, and judging its interior
  would flag a config the spec allows.
- `hooks` must be a mapping (Major), then its keys are validated against
  `KNOWN_HOOK_EVENTS` (Major). Reading only the mapping arm accepted every
  other shape in silence, declaring hooks and wiring none.
- `model` is free-form — aliases and full ids are both valid and the set moves
  with the vendor, the same call the skill validator makes.
- `name` is never checked against the filename: the spec resolves an agent by
  its declared name. Skill names also admit a frontmatter override.
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
- A `SessionStart` matcher's alternatives must be in
  `KNOWN_SESSION_START_SOURCES` (Major). What an alternative IS has one
  owner: `exact_matcher_tokens`, the measured dispatcher grammar — on the
  `KNOWN_MATCHER_EVENTS` the charset is `[a-zA-Z0-9_|, -]` split on `|` and
  `,`, tokens trimmed and empty alternatives dropped, so `startup, resume`
  fires for both and is never flagged, while `startup resume` survives as one
  dead token. A matcher outside its event's charset is an unanchored regex
  matching sources no closed set can enumerate, so membership is not asked of
  it; `*`, the empty string and an absent matcher mean every source. A dead
  alternative is otherwise silent because the surviving alternatives keep the
  hook firing.

  The MCP-matcher audit consumes the same owner, additionally gated to
  `TOOL_QUERY_EVENTS`: on a no-query event the dispatcher consults no matcher
  and every hook fires, so a bare `mcp__server` there is ignored, not a
  no-op — the finding exists only where a tool name is what the matcher
  compares. A hyphen is inside the wide charset, so a hyphenated server name
  is an exact string, never a regex.
- `permissions.deny` empty raises a Minor advisory.
- `permissions.defaultMode` must be in `KNOWN_DEFAULT_MODE_VALUES` if present
  (Major).
- Project / local scope settings carrying a key in
  `KNOWN_PROJECT_SCOPE_NOOP_KEYS` (the const is the owner — see settings.rs;
  `defaultMode: "auto"` is the value-restricted special case) raise a Major
  advisory — those keys silently no-op outside user/managed.
- `skillOverrides` values must be in `KNOWN_SKILL_OVERRIDE_VALUES` (Major).
- Allow rules whose command base is in `DANGEROUS_ALLOW_BASES` without a deny
  of the same base raise a Minor advisory. Match on the normalized base via `bash_base`, which
  collapses the equivalent `cmd:*` / `cmd *` / bare wildcard forms, so both
  spellings are caught and a scoped rule (`rm:./tmp/*`) is not.

Routine validator: shape only, one finding slug (`routine-invalid`) —
`harness_core::routines::ShapeError` for the field grammar, plus the
half-schedule arm (`when` xor `produces` is a lost half-tick, and field
presence is tree shape). Schedule state is deliberately not validated: the
calendar belongs to `harnex lifecycle routines`, never to a gate.

Glob-driven validators (rules / skills / agents / output styles / routines)
implement
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
