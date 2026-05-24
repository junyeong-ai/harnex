# Enforced vs advisory (the organizing principle)

The single axis that decides where a guardrail belongs. Quality scales across
many different developers only through the enforced layer — it is the only
thing that survives a confused, careless, or adversarial agent turn.

## Enforced — deterministic, non-bypassable by the model

| Surface | Why it holds |
|---|---|
| **Hooks** (PreToolUse / PermissionRequest exit 2 or `permissionDecision: deny`) | Run as the client at lifecycle events "regardless of what Claude decides." The only block that a reasoning model cannot talk itself past. |
| **`permissions.deny` / `ask` / `allow`** | Client-enforced; deny wins, first match, merges across scopes. |
| **Managed settings** | Highest precedence, cannot be overridden; org floors (`allowManagedPermissionRulesOnly`, `disableAllHooks`, `strictPluginOnlyCustomization`). |
| **Sandbox** | Filesystem/network isolation for Bash. |

## Advisory — shapes behavior, no guarantee

| Surface | Reality |
|---|---|
| **CLAUDE.md / `.claude/rules/`** | Delivered as a user message after the system prompt — "no guarantee of strict compliance." |
| **Skills** | Instructions + `allowed-tools` pre-approval (grants, not restricts). |
| **Auto-memory** | Model-written notes. |

## The rule harnex generates by

1. **Must-happen → enforced.** Anything that must occur at a point in the
   loop (format-on-edit, block `rm -rf` / secret read, scan before commit)
   becomes a hook or a `permissions.deny` rule — never a CLAUDE.md sentence.
   The memory doc itself redirects: "write it as a hook instead."
2. **Guidance → advisory, minimal, path-scoped.** Architecture intent,
   conventions, where-things-live go in short CLAUDE.md + `.claude/rules/*.md`
   with `paths:` frontmatter so each developer's context stays lean.
3. **Workflow → skill.** Repeatable multi-step procedures (the harnex modes
   themselves) are skills: description costs ~nothing until invoked;
   `disable-model-invocation: true` for side-effectful flows.

## What this means for a generated harness

- The enforced tier is the part that genuinely equalizes quality across a
  team of vibe-coders. Generate it correctly and language-appropriately
  (formatter hook, secret-deny block, destructive-op deny, hook-wrapper
  routing) — this is the highest-leverage output.
- Do not encode an enforcement intent as advisory prose and call it done. A
  "always run lint before committing" line in CLAUDE.md is not enforcement; a
  pre-commit hook is.
- Do not over-fill the advisory tier (see keep-soften-cut.md): every line is a
  recurring per-session token cost and a context-rot contributor.
