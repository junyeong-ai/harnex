# Constitution

Foundation laws. Always loaded (this is the one rule file with no `paths:`).
Imperatives only — rationale lives in commit bodies, not here.

<!-- harnex-managed:start constitution-articles -->
## I. Enforced beats advisory

What must always happen is a hook or a `permissions.deny` rule, never a
sentence in a memory file. CLAUDE.md and rules shape behavior; only hooks and
deny rules are guaranteed.

## II. Secrets never reach git

Two enforced boundaries: the permission layer denies reads/writes/edits of
secret files (stops Claude), and the `hooks/pre-commit` git hook runs
gitleaks on staged changes (stops a developer's own commit). The permission
layer alone cannot cover commits made outside Claude — the git hook closes
that gap. A leaked secret is irreversible once pushed.

## III. Destructive operations are denied, not discouraged

Force-push, hard reset, blanket `git add`, `rm -rf` of roots, and arbitrary
code execution are denied at the permission layer.

## IV. Edits are formatted at the boundary

The formatter runs on every Edit/Write via a PostToolUse hook — style is the
linter's job, never a rule the model must remember.

## V. The session never traps

Stop-class hooks exit 0. For Stop/SubagentStop, exit 2 specifically prevents the
stop and forces continuation (a re-stop loop); findings are surfaced as advisory
context, never as a blocking exit.

## VI. Constraints earn their place

A rule exists only if it enforces an invariant the model cannot self-verify, at
a boundary where a violation is irreversible or invisible. No rule restates a
habit a capable model already follows. No heuristic ships whose false-positive
cost exceeds its catch rate.

## VII. Defaults bend; the floor does not

Two layers, and they are never written alike. **Defaults** are process knobs
the operator owns — review depth, check-in cadence, fan-out, worktree
isolation. An explicit instruction changes them. **The floor** is Articles
I–VI plus whatever guardrail this project made enforced: it bends for nobody
and moves only through break-glass, audited.

Never phrase a default as a floor. An absolute "never" on a cadence or depth
knob reads as rigidity the model honors *over* an explicit operator request,
which costs friction and protects nothing. Reserve "never" for the floor.
<!-- harnex-managed:end constitution-articles -->
