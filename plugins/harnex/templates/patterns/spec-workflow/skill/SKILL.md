---
description: "Spec-driven orchestrator. Runs a change through specify / plan / implement / wrapup, pausing at four gate events and recording each decision. Phase is derived from which artifacts exist, never stored — so a fresh session re-derives where the work stands from disk alone."
when_to_use: "Invoke when a change needs a shape approved before it lands, or will outlive one context window and its state has to live somewhere a later session can read. Also for \"resume\" to pick up an in-flight spec, and to run one phase in isolation. Skip when the work lands green in one pass and the commit body carries everything a later reader needs."
argument-hint: "<what to build> | resume [slug] | <slug> phase=<name>"
disable-model-invocation: true
allowed-tools: Read Write Edit Glob Grep Bash(git log *) Bash(git diff *) Bash(git status *) Bash(git rev-parse *) AskUserQuestion Agent
---

# Spec orchestrator

A spec is a directory under `specs/<slug>/`. It exists for two reasons and
neither is telling the model what to do: someone reviews the shape before the
work lands, and the work outlives the conversation that started it.

`.claude/rules/spec-workflow.md` owns the decision threshold, the artifact
layout, and the lifecycle vocabulary. This skill is the procedure over them.
Read [gates.md](gates.md) when a gate fires and [resume.md](resume.md) on a
resume.

## Phase is derived, never stored

There is no `phase:` field. Which phase the work is in is read off the tree:

| On disk | Phase |
|---|---|
| `spec.md` only | specify |
| `spec.md` + `plan.md`, no implementation commits | plan |
| implementation commits tagged for this slug | implement |
| `wrapup.md` present | wrapup |

A stored phase is a second copy of a fact the directory already states, and it
is the copy that goes stale the moment someone writes a file without updating
it. Deriving costs one `ls` and cannot disagree with reality.

The labels are **descriptive, not a state machine.** Revisit any of them at any
time — a plan that implementation disproves sends the work back to plan, and
that is the process working. What is enforced is the gates, not the order.

## Invocation

| Argument | Mode |
|---|---|
| a description | new spec |
| `resume` | most recent in-flight spec |
| `resume <slug>` | that spec |
| `<slug> phase=<name>` | run one phase only |

Before starting a new spec, list the in-flight ones (`specs/*/spec.md` with a
non-terminal `status`). If any exist, ask whether to resume one, retire one, or
run both — concurrent specs are allowed, and silently starting a second is how
two sessions edit the same files.

## The phases

**specify** — write `specs/<slug>/spec.md`: the problem, the constraints, and
acceptance criteria a later gate can check against. Scan for ambiguity and
resolve it at the `clarify` gate. Acceptance criteria that cannot be checked
are not criteria; rewrite them until they can be.

**plan** — write `plan.md`: the decisions and why, the files this touches, the
task list, and the risks. Findings from any gate land in
`plan.md ## Outstanding Issues`, so the next session reads what the last one
found instead of rediscovering it. A plan crossing <!-- harnex-fill: this
project's blast-radius signals — a migration, a public API, an auth path, a
deploy surface --> fires the `design_review` gate before any code.

**implement** — the tasks, in order, each landing green. Tag every commit that
leaves the spec in flight with its slug so `git log specs/<slug>/` and the
commit trail agree. The `review` gate fires at the end; it is a gate event, not
a phase.

**wrapup** — write `wrapup.md`: what the acceptance criteria actually got,
what was observed on the way, and what is left. Then retire the spec.

## Retiring a spec

A finished spec is state for work that is over. Left in the tree it costs every
future reader the question "is this live?" — which `.claude/rules/artifact-lifecycle.md`
answers for every other artifact and this one is not an exception.

On wrapup or abandonment: write the durable half — what was learned, what a
later reader needs — to <!-- harnex-fill: where this project keeps learnings —
a docs directory, an ADR folder, the commit body if it keeps none -->, retarget
any reference to the spec onto it, and remove the directory. The two differ
only in what the record says. `superseded` and `deprecated` keep the directory:
they are pointers, and a pointer with no target is worse than the file.

A terminal spec is never edited again, and never re-enters a terminal state.

## Never

- **Never ask outside a gate.** The four gate events, the specify ambiguity
  scan, and the concurrency check are the sanctioned moments. One more is
  allowed in plan: a fork that is genuinely the user's — several valid
  approaches, preference-dependent, expensive to reverse, and not answerable
  from the codebase. Most alternatives are not that. Explore and decide them.
- **Never present a bare menu.** Lead with a recommendation and a one-line
  reason. Ask interdependent questions one at a time.
- **Never write a decision token outside the four** `gates.md` defines.
- **Never carry state in the conversation.** Everything a resume needs is on
  disk before the window turns over.
