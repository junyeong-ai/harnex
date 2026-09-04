# Making a change

The constitution says what this codebase is. This says how a change to it is
made. Where the constitution already governs something, this file names it
rather than restating it.

## Order of precedence, when they conflict

1. **Correctness.** A general solution over every valid input, not over the
   test cases. Never hardcode past a failing test. If the requirement is
   unreasonable or the test is wrong, say so rather than route around it.
2. **Proven design.** This repository's existing conventions and structures
   first, then the ecosystem's. Use what is already provided before adding an
   abstraction.
3. **Simplicity.** The least complexity the current requirement needs.

## The next reader is an agent

It has no tribal knowledge, learns from surrounding code as a pattern, and
reads search results rather than files. So these three are the accuracy of the
next change, not decoration:

- **Discoverability.** Match existing naming and architectural boundaries.
  Registrations and entry points stay enumerable by grep. Prefer explicit
  structure to implicit magic — runtime discovery, hidden globals, collection
  by metaclass.
- **Locality.** A fact needed to make a change is found near the change. An
  invariant sits beside the code that enforces it; an assumption and the
  condition that breaks it sit in that code's comment; a test sits near its
  subject.
- **Enforceability.** A convention that repeats becomes a type, a schema, a
  lint, or a test. Prose erodes and gates do not — Article IX is the same rule
  applied to duplicated facts. Before writing a rule down again, check whether
  it can be enforced instead.

"Still right in six months" means the reader can reconstruct the intent then,
not that an extension point was built for it now.

## Scope

Change what was asked and what it directly requires. Do not add comments,
types, or refactors to code left untouched, and do not fold cleanup into a bug
fix. No flexibility, options, or abstraction for a hypothetical future:
speculative generality is harder to remove later than to add later. Prefer a
principled solution to a special-case heuristic or a magic constant. Propose an
adjacent improvement in one line rather than making it.

## Failure

Surface failure explicitly. Never write a fallback, a default, or a swallowed
exception that hides a wrong state — a silent fallback turns a loud bug into a
quiet one. Validate untrusted input at the boundary (user input, external API,
I/O, deserialization) and trust internal calls inside it. Never take a
destructive shortcut or bypass a gate — `--no-verify`, skipped validation,
deleted tests — when blocked.

## Verification

Find the repository's own signals first: tests, lints, types, build commands.
If there are none, say so. Reproduce a defect that crosses a boundary before
fixing it, and read the actual source of the suspect rather than its summary.
A surface symptom, an issue, or a subagent's report is a hypothesis to verify,
never a conclusion to ship. Claim completion with the commands already run and
their output, not with another pass.

A guard claiming that something does not happen is proven by making it happen:
remove what it catches, watch it fail, restore. Green and absent read alike
until then — a pin on a negative, a guard whose own error path swallows the
finding, an arm no fixture reaches. A test asserting a result has that result
as its evidence and is not this case. Put one well-formed change through the
same harness too: a harness that misreports is grading itself, not the guard.
Where the guard is over a set, mutate a member at a time: a whole-set mutant
that kills what one member's mutant kills has said nothing about the rest.

## Commits

One commit, one decision, in a revertible unit. Implement as though it had
always been this way: delete what was replaced in the same change, update every
consumer, and leave no compatibility shim, dead branch, or "used to be" note.
Announce before changing a contract outside this repository — a public API,
schema, config key, or CLI surface. The minor version is that announcement, and
it is the only one an installed harness can act on: `[meta] harnex_version`
pins a range and `Config::validate` enforces it, so a break shipped inside the
pinned range makes the gate state a compatibility that does not hold. Break
freely, and bump the minor in the same release. Supersede a decision record
rather than deleting it.

Record depth follows blast radius: one line for a self-contained reversible
change, and root cause, scope, invariant and verification only where a change
crosses a boundary, alters shared state, or fixes a defect.
