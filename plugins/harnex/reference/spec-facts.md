# Claude Code spec facts (correctness oracle)

The perishable knowledge harnex centralizes. Every generated artifact must
obey these. Re-verify against the live docs before a release; the upstream
surface evolves and freezing it is the failure mode harnex exists to prevent.
Sources: /en/hooks, /en/settings, /en/skills, /en/memory, /en/plugins.

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
  SubagentStop, Notification, PreCompact, PostCompact, InstructionsLoaded,
  ConfigChange, CwdChanged, FileChanged, WorktreeCreate, WorktreeRemove,
  TaskCreated, TaskCompleted, TeammateIdle, Elicitation, ElicitationResult.
  <!-- harnex-managed:end spec-facts-hook-events -->
- **Exit codes.** 0 = success, stdout JSON parsed for control fields (stdout
  reaches Claude as context only for UserPromptSubmit, UserPromptExpansion,
  SessionStart). 1 = non-blocking error, action proceeds. 2 = blocking; stderr
  feeds back to Claude, stdout/JSON ignored. Other = non-blocking.
- **Stop / SubagentStop exit 2 FORCES continuation** (prevents stopping → a
  re-stop loop). A Stop-class wrapper that only wants to surface findings must
  exit 0 and use JSON `decision`/`systemMessage`, never a non-zero exit as a
  generic "found something" signal. Events where exit 2 is genuinely ignored:
  StopFailure, PostToolUse, PostToolUseFailure, PermissionDenied.
- **`timeout` is in SECONDS.** Defaults: 600 (command/http/mcp_tool), 30
  (prompt), 60 (agent); UserPromptSubmit lowers command default to 30. The
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
- **stdin** carries session_id, transcript_path, cwd, permission_mode,
  hook_event_name (PreToolUse adds tool_name, tool_input, tool_use_id).
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
  `Edit(...)`, `Write(...)`, `WebFetch(domain:github.com)`, `Skill(name)`,
  `Agent(Explore)`. MCP uses double-underscore, NOT a parenthesized form:
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
- **Read-only built-ins never prompt** (`ls cat echo pwd head tail grep find wc
  which diff stat du cd` + read-only `git`): an allow rule for them is a no-op —
  never emit one. To force a prompt, add an `ask`/`deny` rule.
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

- **Frontmatter (all optional, only `description` recommended):** name
  (`[a-z0-9-]{1,64}`), description, when_to_use, argument-hint, arguments,
  disable-model-invocation, user-invocable, allowed-tools, model, effort
  (`low|medium|high|xhigh|max`), context (`fork`), agent, hooks, paths, shell.
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
  does NOT restrict.** To restrict, use `permissions.deny`.
- Budgets: description + when_to_use ≤ 1536 chars; SKILL.md ≤ 500 lines (move
  reference to supporting files, loaded on demand).
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
- `@path` import: relative to the importing file, max depth 5, loads at launch.
- Block-level `<!-- ... -->` is stripped before injection (free for notes).
- **CLAUDE.md / rules / auto-memory are ADVISORY** — "no guarantee of strict
  compliance." Only hooks and `permissions.deny` are client-enforced.

## Plugins (/en/plugins)

- Manifest `.claude-plugin/plugin.json`; only `name` required, or omit the
  manifest entirely. Components live at plugin root: `skills/`, `hooks/`,
  `agents/`, `commands/`, `.mcp.json`, `bin/`. A plugin root CLAUDE.md is NOT
  loaded as context — ship instructions in a skill.
- **Omit `version`** for an internal fast-iterating tool: the commit SHA then
  drives updates (every commit is a new version). Set `version` only for
  stable releases users opt into.
- A plugin's own `hooks/hooks.json` runs while the plugin is enabled; it does
  NOT install hooks into a consuming project. harnex therefore does not use
  plugin hooks — it WRITES project-native hook files into `${CLAUDE_PROJECT_DIR}`.
- Install scopes: user / project / local / managed. Distribution via git-hosted
  marketplace (`owner/repo`, any git URL, local path) or `--plugin-dir` for dev.
