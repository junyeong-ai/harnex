# <PROJECT_NAME>

<!-- One-paragraph: what this project is and its primary stack. Keep this
     file under 200 lines and specific — vague guidance reduces adherence. -->

## Layout

<!-- Where things live. One line per top-level area; let the agent read the
     manifest/workspace files for detail rather than restating them here. -->

## Build & test

<!-- The exact commands. Example: `<pm> install`, `<pm> test`, `<pm> build`. -->

## Conventions

<!-- Only project-specific decisions a capable model would not default to.
     Do NOT restate language style the formatter/linter already enforces. -->

<!-- harnex-managed:start enforcement-summary -->
## Enforcement

Guardrails that must always hold live in `.claude/settings.json` (hooks +
`permissions.deny`), not here:
- Secrets and destructive operations are denied.
- Edits are auto-formatted (PostToolUse).
- Sessions surface uncommitted work on Stop without trapping.

See `.claude/rules/constitution.md` for the foundation laws and
`.claude/rules/*.md` (path-scoped) for topic guidance.
<!-- harnex-managed:end enforcement-summary -->
