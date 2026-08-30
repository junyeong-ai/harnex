# Claude Code spec facts (correctness oracle)

The perishable knowledge harnex centralizes. Every generated artifact must
obey these. Re-verify against the live docs before a release; the upstream
surface evolves and freezing it is the failure mode harnex exists to prevent.
Sources: /en/hooks, /en/settings, /en/permissions, /en/skills, /en/memory,
/en/plugins.

## Hooks (/en/hooks)

- **Event surface is a permissive superset, not a fixed count.** Treat the
  known-event list as a typo-catcher, never assert an exact number — the
  surface adds events upstream. Canonical SSoT is
  `crates/harness-core/src/validate/settings.rs::KNOWN_HOOK_EVENTS`; the
  mirror below is held in sync by the `spec_facts_hook_events_match`
  integration test (drift fails the build).
  <!-- harnex-managed:start spec-facts-hook-events -->
  SessionStart, SessionEnd, Setup, UserPromptSubmit, UserPromptExpansion,
  PreToolUse, PostToolUse, PostToolUseFailure, PostToolBatch,
  PermissionRequest, PermissionDenied, Stop, StopFailure, SubagentStart,
  SubagentStop, Notification, MessageDisplay, PreCompact, PostCompact,
  PreModelSwitch, PostModelSwitch,
  InstructionsLoaded, ConfigChange, CwdChanged, DirectoryAdded, FileChanged,
  WorktreeCreate, WorktreeRemove, TaskCreated, TaskCompleted, TeammateIdle,
  Elicitation, ElicitationResult.
  <!-- harnex-managed:end spec-facts-hook-events -->
- **`SessionStart` matcher selects how the session started**, and three of the
  five sources are context-loss boundaries — after `compact`, `clear` or
  `fork` the model holds none of what the hook injected the first time. A
  matcher of `startup|resume` is well-formed and silent at exactly the moments
  its context is worth most, so a hook that injects session state matches all
  five. SSoT is
  `crates/harness-core/src/validate/settings.rs::KNOWN_SESSION_START_SOURCES`.
  <!-- harnex-managed:start spec-facts-session-start-sources -->
  startup, resume, clear, compact, fork.
  <!-- harnex-managed:end spec-facts-session-start-sources -->
- **Exit codes.** 0 = success, stdout JSON parsed for control fields (stdout
  reaches Claude as context only for UserPromptSubmit, UserPromptExpansion,
  SessionStart, PostModelSwitch). 1 = non-blocking error, action proceeds.
  2 = blocking; stderr feeds back to Claude, stdout/JSON ignored. Other =
  non-blocking.
- **Stop / SubagentStop exit 2 FORCES continuation** (prevents stopping → a
  re-stop loop). A Stop-class wrapper that only wants to surface findings must
  exit 0 and use JSON `decision`/`systemMessage`, never a non-zero exit as a
  generic "found something" signal. Events where exit 2 is genuinely ignored:
  StopFailure, PostToolUse, PostToolUseFailure, PermissionDenied.
- **`timeout` is in SECONDS.** Defaults: 600 (command/http/mcp_tool), 30
  (prompt), 60 (agent); UserPromptSubmit, PreModelSwitch and PostModelSwitch
  lower the command default to 30 — and a PreModelSwitch hook cancelled at its
  timeout BLOCKS the model switch (fail-closed), so a slow hook there breaks
  switching rather than degrading quietly. The
  Bash *tool's* `tool_input.timeout` in PreToolUse stdin is milliseconds — a
  different field, opposite unit. Never emit a 4-digit "ms" timeout.
- **Matcher syntax is content-dependent.** `*` / `""` / omitted = match all.
  Only `[A-Za-z0-9_|]` = exact string or `|`-separated list (`Edit|Write` is
  literal-OR, not regex). Any other character makes it a JS regex. An MCP
  server wildcard MUST be `mcp__<server>__.*` — bare `mcp__<server>` matches
  nothing.
- **Config shape:** `hooks → <EventName>[] → { matcher?, hooks[] → { type,
  command, args?, timeout?, ... } }`. Five `type`s: command, http, mcp_tool,
  prompt, agent. `command` is the safe deterministic default for a no-network
  harness. **Reference scripts by `${CLAUDE_PROJECT_DIR}/...` in exec form**
  (`command` = the script, `args` = an array) — a cwd-relative path breaks when
  Claude runs the hook from a subdirectory, and exec form passes each arg
  without shell tokenization (no quoting of spaces). `${CLAUDE_PROJECT_DIR}` /
  `${CLAUDE_PLUGIN_ROOT}` are exported to the spawned process.
- **Common handler fields** (all hook types): `type` (required), `if`
  (permission-rule-syntax predicate — evaluated only on tool events),
  `timeout`, `statusMessage` (shown during execution), `once` (honored in
  skill frontmatter only; ignored in settings files). Command-specific:
  `async` (non-blocking), `asyncRewake`
  (non-blocking + rewake Claude on exit 2 with stderr/stdout as system
  reminder), `shell` (`"bash"` | `"powershell"`).
- **stdin** carries session_id, transcript_path, cwd, permission_mode,
  hook_event_name, effort (PreToolUse adds tool_name, tool_input,
  tool_use_id). Inside subagents: also agent_id, agent_type.
- **`additionalContext`** injects context on SessionStart, Setup,
  SubagentStart, UserPromptSubmit, UserPromptExpansion, and the tool events
  (PreToolUse, PostToolUse, PostToolUseFailure, PostToolBatch) — on tool events
  via `hookSpecificOutput.additionalContext`. It is NOT honored on Stop (use
  `systemMessage` there). Write it as factual statements, not imperatives
  (imperative phrasing trips prompt-injection defenses).

## Settings (/en/settings)

- **Precedence (high→low):** managed → CLI args → local (.claude/settings.local.json)
  → project (.claude/settings.json) → user (~/.claude/settings.json).
- **Permissions evaluate deny > ask > allow, first-match-wins. Arrays MERGE
  (concat + dedupe) across scopes — they do not override.** An `allow` cannot
  loosen a higher-scope `deny`. With no matching rule, `default` mode PROMPTS
  (asks) — it is not a hard-deny; hard-deny is the opt-in `dontAsk` mode.
- **Silently ignored in project/local settings** (set only in user/managed).
  Canonical SSoT is `KNOWN_PROJECT_SCOPE_NOOP_KEYS` in
  `crates/harness-core/src/validate/settings.rs`; the mirror below is held
  in sync by the `spec_facts_noop_keys_match` integration test.
  <!-- harnex-managed:start spec-facts-project-scope-noop-keys -->
  autoMemoryDirectory, autoMode, useAutoModeDuringPlan,
  skipDangerousModePermissionPrompt, claudeMd.
  <!-- harnex-managed:end spec-facts-project-scope-noop-keys -->
  (`defaultMode: "auto"` is a VALUE restriction, not a key restriction —
  handled separately by the `SettingsScope` check.) Never emit these into a
  generated `.claude/settings.json` — they become no-ops.
- **Pattern syntax:** `Bash(npm run *)`, `Read(.env)`, `Read(./secrets/**)`,
  `Edit(...)`, `PowerShell(Get-ChildItem *)`, `WebFetch(domain:github.com)`,
  `Skill(name)`, `Agent(Explore)`.
  MCP uses double-underscore, NOT a parenthesized form:
  `mcp__<server>` (all its tools), `mcp__<server>__<tool>`, or `mcp__<server>__*`.
- **Bash matching:** a single `*` matches any run of characters *including
  spaces*, so one wildcard spans multiple args. The canonical wildcard is a
  space then `*` (`Bash(ls *)` matches `ls -la` but NOT `lsof` — word boundary);
  `Bash(ls*)` (no space) also matches `lsof`. The `:*` suffix is an *equivalent*
  trailing wildcard (`Bash(ls:*)` ≡ `Bash(ls *)`) recognized ONLY at the end —
  mid-pattern `:` is literal. Wildcards may appear at any position
  (`Bash(* --version)`). Wrappers `timeout/time/nice/nohup/stdbuf` (and bare
  `xargs`) are stripped before matching; env-runners (`npx`, `docker exec`,
  `devbox run`) are NOT — write `Bash(npx <tool> *)`, never bare `Bash(npx *)`.
- **`PowerShell` is a second shell tool, not a Windows detail.** It is enabled
  by default on Windows without Git Bash and opt-in elsewhere
  (`CLAUDE_CODE_USE_POWERSHELL_TOOL=1` + `pwsh` on PATH), so a `Bash`-only rule
  set leaves it ungoverned wherever it is on. Two consequences: a hook that
  inspects shell commands matches `Bash|PowerShell`, never `Bash` alone; and a
  deny floor that mirrors only Bash states a narrower boundary than it reads as.
  harnex's baseline deliberately stays Bash-only — the generated harness targets
  POSIX toolchains where the tool is off by default, and mirroring every rule
  for a tool no target project enables is cost without catch. A project that
  turns it on owns the mirror.
- **Read-only built-ins never prompt** (`ls cat echo pwd head tail grep find wc
  which diff stat du cd` + read-only `git`): an allow rule for them is a no-op —
  never emit one. To force a prompt, add an `ask`/`deny` rule.
- **File permission checks consult `Read(path)` and `Edit(path)` only**, and
  `Edit` covers every built-in tool that edits files. A path rule written for
  any tool below is accepted, merges across scopes, and is never read — it
  reads as a floor and enforces nothing, and Claude Code warns at startup
  (`… is not matched by file permission checks`). Canonical SSoT is
  `crates/harness-core/src/policy/rule.rs`; the mirrors below are held in sync
  by the `spec_facts_*` integration tests, and both `harnex validate settings`
  and `harness.toml` load reject such a rule where it is written.
  Path rules `Edit(...)` owns:
  <!-- harnex-managed:start spec-facts-covered-by-edit-rules -->
  MultiEdit, NotebookEdit, Write.
  <!-- harnex-managed:end spec-facts-covered-by-edit-rules -->
  Path rules `Read(...)` owns:
  <!-- harnex-managed:start spec-facts-covered-by-read-rules -->
  Glob.
  <!-- harnex-managed:end spec-facts-covered-by-read-rules -->
  A bare tool-name rule with no path (`"Write"`) is a tool-level rule, matches
  everywhere, and is never warned about — `Tool(*)` is the same rule. A `Read`
  deny also blocks Edit and Write on that path, but NOT NotebookEdit, so a
  path no tool may change needs the `Edit` deny in its own right.
- **`Tool(param:value)` matches one top-level input parameter** on a deny/ask
  rule (`Agent(model:opus)`, `Bash(run_in_background:true)`); allow rules keep
  each tool's own specifier syntax. The value takes `*`; an omitted parameter
  never matches. Each tool's primary content field is refused — a content
  match is bypassable by a compound command — so a rule naming one is ignored
  and warned about:
  <!-- harnex-managed:start spec-facts-primary-content-fields -->
  Bash:command, Edit:file_path, Glob:path, Grep:path,
  NotebookEdit:notebook_path, PowerShell:command, Read:file_path,
  WebFetch:url, Write:file_path.
  <!-- harnex-managed:end spec-facts-primary-content-fields -->
- **Read/Edit denies extend to Bash file commands** `cat`/`head`/`tail`/`sed`,
  so `Read(.env)` deny already blocks `cat .env` — no `Bash(cat …)` mirror
  needed. They do NOT reach arbitrary subprocesses (a Python/Node script).
- **Read/Edit path anchors (gitignore semantics):** bare `path` or `./path` =
  cwd-relative; `/path` = project-root; `//path` = filesystem-absolute; `~/path`
  = home. A bare filename matches at any depth — `Read(.env)` ≡ `Read(**/.env)`,
  so the `**/` mirror is redundant. `*` = one path segment, `**` = recursive.
- **`skillOverrides` values:** `on` | `name-only` | `user-invocable-only` |
  `off` (absent = `on`). `autoMemoryEnabled`: bool, default true.
- Managed-scope enforcement: two distinct tiers, do not conflate.
  *Managed-ONLY floors* (only the managed value is honored):
  `allowManagedPermissionRulesOnly`, `allowManagedHooksOnly`,
  `strictPluginOnlyCustomization`. *Strongest-from-managed* (settable at other
  scopes too, but managed wins / cannot be overridden there):
  `disableAllHooks`, `disableSkillShellExecution`, `sandbox` (per-subkey).
  `claudeMd` is managed/policy-only memory content (not an enforcement floor) —
  it no-ops at project/local (see the no-op-keys list above).

## Skills (/en/skills)

- **Frontmatter (all optional, only `description` recommended).** Canonical
  SSoT is `crates/harness-core/src/validate/skills.rs::KNOWN_SKILL_KEYS`;
  the mirror below is held in sync by the `spec_facts_skill_keys_match`
  integration test (drift fails the build).
  <!-- harnex-managed:start spec-facts-skill-keys -->
  name, description, when_to_use, argument-hint, arguments,
  disable-model-invocation, user-invocable, allowed-tools, disallowed-tools,
  model, effort, context, agent, background, hooks, paths, shell, metadata,
  license, compatibility.
  <!-- harnex-managed:end spec-facts-skill-keys -->
  Constraints: name (`[a-z0-9-]{1,64}`), effort (`low|medium|high|xhigh|max`),
  context (`fork`), background (bool, only with `context: fork`, ≥ v2.1.218),
  metadata (a mapping — any other value is silently dropped). `license` and
  `compatibility` come from the Agent Skills standard: accepted, never acted on.
- **Location:** `.claude/skills/<name>/SKILL.md` (project/personal),
  `<plugin-root>/skills/<name>/SKILL.md` (plugin), or — since v2.1.142 — a bare
  `SKILL.md` at the plugin root with no `skills/` dir and no `skills` manifest
  field, which loads as a single-skill plugin (invocation name from frontmatter
  `name`, else directory basename). Plugin skills are namespaced `plugin:skill`.
- **`disable-model-invocation: true`** is the field that blocks programmatic
  (Claude-triggered) invocation and hides the description from context — use it
  for side-effect skills (generate/write/deploy). `user-invocable: false` only
  hides the menu; it does NOT block the Skill tool.
- **`allowed-tools` GRANTS (pre-approves) tools while the skill is active; it
  does NOT restrict.** `disallowed-tools` REMOVES tools from Claude's pool
  while the skill is active (the inverse). To deny outright, use
  `permissions.deny`.
- Budgets: description + when_to_use ≤ 1536 chars; SKILL.md ≤ 500 lines (move
  reference to supporting files, loaded on demand). After compaction,
  skill content keeps first 5 000 tokens/skill and 25 000 tokens combined
  (most-recent-first).
- **Dynamic context injection:** `` !`command` `` in SKILL.md body runs a shell
  command before content reaches Claude; output replaces the placeholder.
  Disabled per-project by `disableSkillShellExecution: true`.
- **Bundled-asset variables:** `${CLAUDE_SKILL_DIR}` — the directory holding
  this skill's `SKILL.md`; the documented, install-level-portable anchor for
  skill-bundled reference docs and templates (works whether installed
  personal / project / plugin). `${CLAUDE_PROJECT_DIR}` — the target repo root,
  where generated files are written. `${CLAUDE_PLUGIN_ROOT}` is the plugin-root
  anchor (equal to the skill dir for a single-skill-at-root plugin); prefer
  `${CLAUDE_SKILL_DIR}` for skill-owned files.

## Memory (/en/memory)

- **CLAUDE.md** loads broad→specific, concatenated (not overriding): managed →
  user → project (`./CLAUDE.md` or `./.claude/CLAUDE.md`) → local
  (`CLAUDE.local.md`). Within the project tree it walks ancestors from cwd
  upward and orders them root→cwd (so the deepest, closest file is read last);
  within each directory `CLAUDE.local.md` is appended after `CLAUDE.md`.
  Subdir CLAUDE.md (below cwd) loads lazily when Claude reads files there.
- **Target ≤ 200 lines** per file; longer reduces adherence.
- **Path-scoped rules:** `.claude/rules/*.md`; with `paths:` frontmatter (glob,
  brace expansion) they load only on matching files; without `paths:` they load
  every session. A foundation rule (constitution) is the one that intentionally
  omits `paths:`.
- `@path` import: relative to the importing file, max depth 4, loads at launch.
- **`claudeMdExcludes`:** glob patterns to skip specific CLAUDE.md files.
  Merges across settings layers. Managed-policy files cannot be excluded.
- Block-level `<!-- ... -->` is stripped before injection (free for notes).
- **CLAUDE.md / rules / auto-memory are ADVISORY** — "no guarantee of strict
  compliance." Only hooks and `permissions.deny` are client-enforced.

## Plugins (/en/plugins)

- Manifest `.claude-plugin/plugin.json`; only `name` required, or omit the
  manifest entirely. Components live at plugin root: `skills/`, `commands/`,
  `agents/`, `workflows/`, `output-styles/`, `hooks/hooks.json`, `.mcp.json`,
  `.lsp.json`, `bin/`, plus the experimental `themes/` and
  `monitors/monitors.json`. Beyond the metadata fields, the manifest carries
  `userConfig` (typed values an installer supplies, `sensitive` ones through
  secure storage), `dependencies` (other plugins, optional semver), `channels`,
  and `defaultEnabled`. A plugin root CLAUDE.md is NOT loaded as context —
  ship instructions in a skill.
- **A plugin agent is namespaced `plugin:agent`, like a skill.** Verified by
  installing with `--plugin-dir` and asking a running session: an agent whose
  frontmatter says `name: session-judge` resolves as `harnex:session-judge`,
  and the bare name resolves to nothing. A command that dispatches the bare
  name fails at the point it dispatches and nowhere earlier.
- **`claude plugin details <name>` reports what the runtime actually
  discovered** — component counts and a projected always-on token cost — and
  counts `commands/` entries alongside `skills/` in that inventory. It is the
  cheapest check that a plugin's assets are found at all, and it needs the
  top-level `--plugin-dir` flag rather than a subcommand option.
- **`workflows/` is a distinct component class from `skills/`.** A skill is
  instructions a model follows; a workflow is a script the runtime executes
  over many subagents, invoked as a slash command. The two are not
  substitutes: a workflow earns its place where one charge runs identically
  over many items whose defects are independent, and where no step needs
  operator input mid-run.
- **Omit `version`** for an internal fast-iterating tool: the commit SHA then
  drives updates (every commit is a new version). Set `version` only for
  stable releases users opt into.
- A plugin's own `hooks/hooks.json` runs while the plugin is enabled; it does
  NOT install hooks into a consuming project. harnex therefore does not use
  plugin hooks — it WRITES project-native hook files into `${CLAUDE_PROJECT_DIR}`.
- Install scopes: user / project / local / managed. Distribution via git-hosted
  marketplace (`owner/repo`, any git URL, local path) or `--plugin-dir` for dev.
