# Retire — removing what the operator decides is not earning its place

`extend` adds. Nothing removed, so a harness only ever grew: a Stop hook
spending 2.6 seconds per stop stayed because there was no verb that could take
it out. This is that verb menu.

## Evidence is a candidate, never a verdict

`lifecycle` already settled this shape and refuses to auto-retire for the same
reason it applies here: what a harness element **costs** is recorded, and what
it **bought** is not.

The transcript is precise about cost. Every Stop event records each hook's
`durationMs` separately, so `total_ms` is exact per hook. It is silent about
value, and not by omission — the fields that would carry it belong to the event
rather than to a hook inside it. `preventedContinuation`, `hasOutput` and
`hookAdditionalContext` describe the Stop, and every Stop in the measured
corpus ran between three and six hooks (single-hook stops: 0 of 7,195), so none
of them resolves to one hook even in principle.

`stops_with_prevention` is therefore not a predicate. Zero means this hook
never held the agent — which is also true of every hook that was never meant
to, and a Stop wrapper that reports rather than blocks exits 0 by design. A
removal gated on that zero would mark every hook in the corpus, including the
gates that are working.

So this mode presents evidence and refuses to act on it alone. The operator
supplies the decision, and the decision is recorded with the evidence beside
it.

## Invariants — every verb, no exceptions

1. **Evidence alone never removes anything.** The oracle supplies cost and
   absence; the operator supplies the reason. `harness lifecycle` refuses empty
   decision text, and so does this.
2. **The window must have seen the project.** Absence of evidence in a window
   that never observed this project is not evidence of absence. Each verb
   states how it establishes the window saw the project.
3. **harnex removes only what harnex owns.** An entry outside the managed
   partition (SKILL.md § Invariants) is reported with its location and left
   alone. A hook in `~/.claude/settings.json` is outside `${CLAUDE_PROJECT_DIR}`
   and therefore always a report — which is where the largest cost in the
   measured corpus lives.
4. **One revertible commit.** A removal lands alone, so `git revert` restores
   it without carrying anything else back.
5. **State the limit with the number.** Present cost as cost. Do not describe a
   hook as useless because nothing recorded it being useful.

## `retire drop-hook <command>`

**Evidence:** the hook's row in `facts.harness.hooks` — `runs`, `total_ms`, and
`stops_with_prevention` with the caveat above.

**Window (invariant 2):** the hook appears in `facts.harness.hooks` at all; a
hook the window never saw run has no row.

**Locating the entry.** `facts` reports the command as the runtime rendered it.
A settings entry renders as its `command`, plus a space and its `args` joined
by spaces when `args` is present. Match by rendering each entry and comparing
for exact equality:

- exactly one entry matches → the removal target
- no entry matches → report. The hook may come from a plugin
  (`${CLAUDE_PLUGIN_ROOT}`), from a settings scope outside the project, or from
  an `args` shape this rendering does not reproduce. Do not relax the
  comparison to find it — a near-match removes the wrong hook.
- more than one matches → report both and stop.

A failed match is a false negative: a removable hook goes unoffered. That is
the direction this comparison is allowed to fail in, and the reason exact
equality is used rather than anything looser.

## `retire drop-rule <path>`

**Evidence:** the rule's absolute path does not appear in
`facts.harness.rule_loads` — it entered context zero times across the window.

**Window (invariant 2):** at least one rule of this project appears in
`rule_loads`. If none do, the window carries no evidence about this project and
every rule would look unloaded. Refuse and say so.

**The limit to state (invariant 5):** a path-scoped rule loads only when a file
it governs is read. A window in which nothing it governs was touched is a
window in which it correctly did nothing. Report which of the project's rules
did load, so the operator can see whether the window exercised this rule's
scope at all.

## What is not here, and why

- **A rule that loads and is violated anyway.** It wants a rewrite, not a
  removal, and the evidence needs defect data this module does not have.
- **Shrinking a rule.** The oracle can say a rule costs 47,669 characters per
  load; it cannot say which section is the cost. Report the figure and let the
  operator edit — that is `extend` territory, not a removal.
- **Softening a permission rule.** A denial resolves to the tool it refused,
  never to the rule that refused it: the record does not carry one. Report
  `permission-rule` denials by tool and stop there.
- **Retiring a skill.** No evidence of an unused skill has been observed yet.
  When it is, the verb follows the same shape.
