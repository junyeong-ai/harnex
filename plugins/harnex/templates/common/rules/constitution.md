# Constitution

Foundation laws. Always loaded: this rule carries no `paths:`.
Imperatives only — rationale lives in commit bodies, not here.

<!-- harnex-managed:start constitution-articles -->
## I. Enforced beats advisory

What must always happen is a hook or a `permissions.deny` rule, never a
sentence in a memory file. CLAUDE.md and rules shape behavior; only hooks and
deny rules are guaranteed.

## II. The floor is read where it runs

Secrets, destructive operations, formatting and the end of a session are held
by `permissions.deny` and the hooks in `.claude/settings.json`, and by
`hooks/pre-commit`. Read those files for what is blocked. A Stop-class hook
that reports exits 0: exit 2 keeps the session from stopping.

## III. Constraints earn their place

A rule exists only if it enforces an invariant the model cannot self-verify, at
a boundary where a violation is irreversible or invisible. No rule restates a
habit a capable model already follows. No heuristic ships whose false-positive
cost exceeds its catch rate.

## IV. Defaults bend; the floor does not

Two layers, and they are never written alike. **Defaults** are process knobs
the operator owns — review depth, check-in cadence, fan-out, worktree
isolation. An explicit instruction changes them. **The floor** is Articles
I–III plus whatever guardrail this project made enforced: it bends for nobody
and moves only through break-glass, audited.

Never phrase a default as a floor. An absolute "never" on a cadence or depth
knob reads as rigidity the model honors *over* an explicit operator request,
which costs friction and protects nothing. Reserve "never" for the floor.
<!-- harnex-managed:end constitution-articles -->
