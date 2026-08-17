# <!-- harnex-fill: the project name, from the manifest -->

<!-- harnex-fill: one paragraph — what this project is and its primary stack -->

## Layout

<!-- harnex-fill: where things live, one line per top-level area — let the agent read the manifests for detail -->

## Build & test

<!-- harnex-fill: the exact gate commands in the project's own declared order, each as `<command>` — `<what it does>` -->

## Conventions

<!-- harnex-fill: only decisions the formatter and linter do not already enforce -->

<!-- harnex-managed:start enforcement-summary -->
## Enforcement

Guardrails that must always hold live in `.claude/settings.json` (hooks +
`permissions.deny`), not here:
- Secrets and destructive operations are denied.
- Edits are auto-formatted (PostToolUse).
- Sessions surface uncommitted work on Stop without trapping.

See `.claude/rules/constitution.md` for the foundation laws,
`.claude/rules/agent-conduct.md` for how to work in this repo, and
`.claude/rules/*.md` (path-scoped) for topic guidance.
<!-- harnex-managed:end enforcement-summary -->
