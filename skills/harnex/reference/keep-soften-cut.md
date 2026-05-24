# Keep / soften / cut (avoiding valueless constraints)

Modern models (Opus 4.x-class) make many traditional harness heuristics
valueless or harmful. A generated harness must not impose them.

## Governing principle

> Enforce only invariants the model cannot self-verify from inside its own
> context window, at the point where a violation becomes irreversible or
> invisible. Never encode guidance a capable model already follows by default.
> Never deploy a heuristic whose false-positive cost (a correct edit gets
> blocked or nagged) exceeds its catch rate against a defect the model would
> actually commit.

Corollaries: boundary test, not behavior coaching · non-bypassability is the
only thing the model can't give itself · substring/regex/edit-distance over
prose or source has a false-positive floor → advisory/warn-only at best, never
a blocking gate. Research backing: over-constraining capable models lowers
output quality (constraint-decay); long prescriptive review checklists raise
false positives (systematic overcorrection); excess context degrades all
frontier models (context rot).

## KEEP — enforce (low false-positive, model-unverifiable)

- Non-bypassable runtime guards: hooks, `permissions.deny` of destructive ops
  (`rm -rf` roots, force-push, `reset --hard`, cloud-destroy verbs), atomic /
  traversal-safe write paths, bounded Stop-audit retry counter.
- Closed-set / exact-match membership: hook event names (as a typo-catcher),
  permission-profile names, commit-msg closed-enum trailers (parsed as the
  last blank-line-delimited block, never the subject/body).
- Referential integrity: a cited path/symbol/glossary key resolves; an import
  stays within the allowed dependency boundary (AST, not substring).
- Secret scanning at commit (gitleaks) — irreversible once pushed.
- Load-time config validation (fail-at-load, pure structural).

## SOFTEN — advisory / opt-in, escape hatch mandatory

- Numeric caps (line counts) — a cohesive 210-line file is not a defect; frame
  as "review for domain mixing," not auto-fail.
- Side-effect *verb* detection over a description — matches prose, not intent;
  a model judges "does this skill perform the side effect" better than `\bsend\b`.
- Unknown-frontmatter-key rejection — valuable but a hardcoded key list lags
  the upstream spec; keep opt-in, default off, with the full spec surface.
- Any regex/substring over source (await-wrapping, `throw new Error`) — keep
  only with an `allow:`-marker escape hatch.

## CUT — emit nothing

- Restating model-default coding habits the formatter already owns: import
  ordering, mutable-default-args, naming case, `print` vs logger, quote style.
  The linter (ruff/biome) is the SSoT; do not duplicate it in prose.
- Human-pedagogical "Why" essays in always-loaded files. Keep a one-line
  motivation (Claude generalizes better from a brief reason), move the
  rationale to the commit body / decision record. Do not spend per-session
  context re-explaining.
- Info findings for a correct, intentional config (e.g. "we noticed you set
  autoMemoryEnabled"). A finding that never implies action should not exist.
- Fossils guarding obsolete failure modes (tool-call-payload leakage into
  committed files) — near-zero catch rate against a current model.
- Long prescriptive review checklists — fewer high-confidence checks; let
  `effort` scale breadth, not a fixed maximal list.

## Prompt discipline for the advisory tier

Specific-but-minimal. Capable models punish vagueness (they execute literally)
AND verbosity/rigidity. Emit concrete, verifiable instructions; never terse
"infer my intent," never a wall of rules the model must navigate mid-generation.
