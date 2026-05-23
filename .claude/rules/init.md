---
paths:
  - "crates/harness-core/src/init/**"
  - "crates/harness-core/templates/**"
---

# init — project scaffolder

`ProjectInitializer` writes 5 core files plus 5 hook scripts to a
target directory. Core files: `harness.toml`, `CLAUDE.md`, `README.md`,
`.claude/rules/constitution.md`, `.claude/settings.json`. Hook scripts
(in `hooks/`): `_runner.sh`, `_stop_runner.sh`, `post-format.sh`,
`session-start.sh`, `check-on-stop.sh`. Templates live in
`crates/harness-core/templates/` and are embedded via `include_str!`.

Hook generation is enabled by default (`with_hooks(true)`). Disable via
`with_hooks(false)` or `--no-hooks` CLI flag. When hooks are enabled,
the generated `.claude/settings.json` includes a `hooks` section wiring
SessionStart, PostToolUse, and Stop events to the runner scripts. Hook
files are set executable (0o755) on Unix after creation.

Self-consistency invariant: the generated `harness.toml` MUST pass
`Config::load + validate`. Test `generated_harness_toml_loads_and_validates`
asserts this round-trip and fails any template that breaks it.

The `.claude/settings.json` permission block is generated at write time
from `policy::PermissionGenerator` with the `baseline` profile — single
SSoT, never a hand-maintained mirror.

Existing files are skipped unless `force=true`. Templates contain
`<PROJECT_NAME>` placeholder which is substituted from the constructor
arg. If `AGENTS.md` exists at target root, the generated `CLAUDE.md`
appends `@AGENTS.md` import line (per Claude Code memory spec's
recommended interop pattern).

When adding a new scaffolded file:
1. Add the template under `crates/harness-core/templates/`.
2. Wire it into `ProjectInitializer::run`'s `plans` vector.
3. Add a test verifying creation + skip-on-existing + force-overwrite.
