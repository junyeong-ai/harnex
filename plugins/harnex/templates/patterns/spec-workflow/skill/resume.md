# Resume

A session ends mid-spec — compaction, a closed window, a different day. Resume
re-derives where the work stands. It never asks the previous session what it
was doing, because the previous session is gone and its answer was never state.

## What state is

Three sources, and nothing else:

1. **`spec.md` frontmatter** — `status`, `created`, `updated`, and the
   supersession links. The single source for lifecycle.
2. **Artifact presence** — which of `spec.md` / `plan.md` / `wrapup.md` exist.
   This is what the phase is derived from; there is no stored phase.
3. **`git log`** — the timeline, and which commits carry this slug.

Anything held only in the conversation is not state. If a session is about to
end with something that matters, it goes into one of the three first.

## The procedure

1. Pick the spec: the named slug, or the most recently updated non-terminal one.
2. Derive the phase from artifact presence.
3. Read `plan.md ## Outstanding issues` — this is what the last pass found, and
   re-finding it is the waste this file exists to prevent.
4. Check the worktree. Dirty fires the `resume` gate.
5. Continue from the derived phase.

## Compaction is a checkpoint

This harness compacts and the work continues. Never wind a spec down early to
conserve context, and never stop at a phase boundary because the window is
filling — land the unit in flight, put its state on disk, and let the window
turn over.

Starting a fresh window and resuming is an equal option to compacting, and the
better one once the conversation has accumulated approaches that were abandoned
and readings that were superseded. Both re-derive the same position from the
same three sources. Only one of them pays to carry what it will not use.
