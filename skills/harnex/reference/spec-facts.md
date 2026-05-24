# Claude Code spec facts (correctness oracle)

The perishable knowledge harnex centralizes. Every generated artifact must
obey these. Re-verify against the live docs before a release; the upstream
surface evolves and freezing it is the failure mode harnex exists to prevent.
Sources: /en/hooks, /en/settings, /en/skills, /en/memory, /en/plugins.

## Hooks (/en/hooks)

- **Event surface is a permissive superset, not a fixed count.** Treat the
  known-event list as a typo-catcher, never assert an exact number — the
  surface adds events upstream. Current named events include: SessionStart,
  Setup, UserPromptSubmit, UserPromptExpansion, PreToolUse, PermissionRequest,
  PermissionDenied, PostToolUse, PostToolUseFailure, PostToolBatch,
  Notification, SubagentStart, SubagentStop, TaskCreated, TaskCompleted, Stop,
  StopFailure, TeammateIdle, InstructionsLoaded, ConfigChange, CwdChanged,
  FileChanged, WorktreeCreate, WorktreeRemove, PreCompact, PostCompact,
  Elicitation, ElicitationResult (SessionEnd appears in some tables).
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
  command, timeout?, ... } }`. Five `type`s: command, http, mcp_tool, prompt,
  agent. `command` is the safe deterministic default for a no-network harness.
- **stdin** carries session_id, transcript_path, cwd, permission_mode,
  hook_event_name (PreToolUse adds tool_name, tool_input, tool_use_id).
- **`additionalContext`** is honored only on SessionStart, Setup, SubagentStart,
  UserPromptSubmit, UserPromptExpansion. Write it as factual statements, not
  imperatives (imperative phrasing trips prompt-injection defenses).

## Settings (/en/settings)

- **Precedence (high→low):** managed → CLI args → local (.claude/settings.local.json)
  → project (.claude/settings.json) → user (~/.claude/settings.json).
- **Permissions evaluate deny > ask > allow, first-match-wins, then
  default-deny. Arrays MERGE (concat + dedupe) across scopes — they do not
  override.** An `allow` cannot loosen a higher-scope `deny`.
- **Silently ignored in project/local settings** (set only in user/managed):
  `defaultMode: "auto"`, `autoMemoryDirectory`, `autoMode`,
  `skipDangerousModePermissionPrompt`. Never emit these into a generated
  `.claude/settings.json` — they become no-ops.
- **Pattern syntax:** `Bash(npm run *)`, `Read(./.env)`, `Read(./secrets/**)`,
  `Edit(...)`, `Write(...)`, `WebFetch(domain:github.com)`, `MCP(server)`,
  `Skill(name)`. `*` = one segment, `**` = recursive; `/` project-relative,
  `//` absolute, `~/` home.
- **`skillOverrides` values:** `on` | `name-only` | `user-invocable-only` |
  `off` (absent = `on`). `autoMemoryEnabled`: bool, default true.
- Managed-only enforcement floors: `allowManagedPermissionRulesOnly`,
  `allowManagedHooksOnly`, `disableAllHooks`, `disableSkillShellExecution`,
  `strictPluginOnlyCustomization`, `claudeMd`, `sandbox`.

## Skills (/en/skills)

- **Frontmatter (all optional, only `description` recommended):** name
  (`[a-z0-9-]{1,64}`), description, when_to_use, argument-hint, arguments,
  disable-model-invocation, user-invocable, allowed-tools, model, effort
  (`low|medium|high|xhigh|max`), context (`fork`), agent, hooks, paths, shell.
- **Location:** `<plugin-root>/skills/<name>/SKILL.md` (plugin) or
  `.claude/skills/<name>/SKILL.md` (project). Plugin skills are namespaced
  `plugin:skill`.
- **`disable-model-invocation: true`** is the field that blocks programmatic
  (Claude-triggered) invocation and hides the description from context — use it
  for side-effect skills (generate/write/deploy). `user-invocable: false` only
  hides the menu; it does NOT block the Skill tool.
- **`allowed-tools` GRANTS (pre-approves) tools while the skill is active; it
  does NOT restrict.** To restrict, use `permissions.deny`.
- Budgets: description + when_to_use ≤ 1536 chars; SKILL.md ≤ 500 lines (move
  reference to supporting files, loaded on demand).
- **Bundled-asset variables:** `${CLAUDE_PLUGIN_ROOT}` (plugin install dir —
  reference templates/scripts), `${CLAUDE_PROJECT_DIR}` (target repo root —
  where generated files are written), `${CLAUDE_PLUGIN_DATA}` (persistent
  state). `${CLAUDE_SKILL_DIR}` is not documented for plugins; use
  `${CLAUDE_PLUGIN_ROOT}`.

## Memory (/en/memory)

- **CLAUDE.md** loads broad→specific, concatenated (not overriding): managed →
  user → project (`./CLAUDE.md` or `./.claude/CLAUDE.md`) → local
  (`CLAUDE.local.md`). Subdir CLAUDE.md loads on demand.
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
