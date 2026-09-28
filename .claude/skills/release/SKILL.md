---
description: "Take a change from converged review to a proven release: two independent whole-surface reviews, the version decision, the gate run, tag, push, CI, assets, and a proof against the released binary. Reads the workspace gates, `.github/workflows/ci.yml`, and `gh run`; writes the version bump, the tag, and the release."
when_to_use: "At a milestone — work is complete and about to leave this repository. Not for an ordinary commit, which needs the gates and nothing here. Manual only: it tags and pushes, and neither is undone by re-running."
disable-model-invocation: true
allowed-tools: Read Edit Bash(cargo *) Bash(harnex *) Bash(git *) Bash(gh *) Bash(claude plugin *) Bash(npm ci *) Bash(npx playwright *) Bash(scripts/mutants.sh *)
---

# release

## 1 — Converge the review before deciding anything else

Two reviewers over the whole changed surface, dispatched in parallel and
**given no access to each other's findings** — a shared list turns the second
reviewer into a confirmer. One reads with no authoring context; the other is a
separate engine, so a blind spot has to be shared by both to survive.

Then, for every claim either returns:

- **Reproduce it against the built binary before fixing it.** A reviewer's
  finding is a hypothesis. A fix landed on an unreproduced claim is a change
  with no defect behind it.
- **Mutation-test every guard the change adds or touches**, to the discipline
  `making-changes.md § Verification` states: put in what the guard catches,
  run the tests that name it, restore. A guard that passes its own mutation is
  watching nothing, and the suite still reports green. A mutant is caught
  only when its test filter passes unmutated and fails with a test failure
  once mutated: a build error or an argument the runner rejects fails the
  same command and says nothing about the guard. Restore with a fresh mtime,
  or cargo keeps the mutant's build and the next baseline runs it. A missed
  mutant is an open claim until a test kills it.
- **`scripts/mutants.sh --in-diff <file> --test-workspace=false
  --copy-vcs=true` computes the set over a whole range** when the range
  warrants one; at roughly half a minute a mutant on this workspace, a range
  of two hundred runs for hours. The git directory is copied because integration tests read it,
  and without it the unmutated baseline fails before any mutant runs. The
  wrapper is what bounds a mutant's memory; `cargo mutants` bounds only its
  wall clock, and a mutant of a loop that allocates per turn takes the machine
  before that comes round. One another package's test would kill is confirmed
  by re-running it with `--test-workspace=true`.

Do not start §3 while a claim is open. An unresolved finding after a tag is a
finding that ships, and a review dispatched and not yet read is an open claim —
two is the floor on reviewers, so a third still running is not a spare.

## 2 — Decide the version

`making-changes.md` owns the rule: the minor is how a break in a contract
outside this repository is announced, and it ships in the same release as the
break. Decide which this is before bumping, and say why in the release commit.

Gate output on unmodified input is on that list, and it is the surface a
release here moves most often. A finding newly reported because it was always
true is the gate catching up rather than a break. Everything else is a break, a
withdrawal included — and one withdrawal is worth shipping as a patch anyway:
the one repairing a regression the pinned range already shipped, where the
minor announcing it would put a pin edit between an operator and that repair.
A withdrawal taken for cost repairs nothing and findings only stop, and one of
behaviour operators have held for minors is no different; both are breaks like
the rest. Say which it is and why. A second withdrawal of the same behaviour is
a design being litigated in releases rather than decided.

Diff the gate over a corpus before and after, and over one that can produce the
change: a corpus that never reaches the changed code diffs clean whatever the
change did. The diff is mechanical; reading each flip as a
true or a false finding is not, and that reading is the decision.

## 3 — Run the chain

Run the gates so **their exit status is the run's**. A command substitution
returns a gate's output rather than its status, so a chain built on one runs
on past a suite that failed.

Every CI job needs a local counterpart before a tag, and
`.github/workflows/ci.yml` is the list to check that against. Two kinds of gap,
and only one of them is a hole. A job whose twin was never written is a hole:
a drift only that job reads ships unseen, which is what `schema_sync.rs`
answers for the schema-drift job. A job that cannot run here is not a hole: the
test matrix carries two operating systems and a development machine is one of
them, so the other leg is only ever green in CI. That is why the run is watched
rather than predicted.

The pins are not a manual sweep, but they take two gates and not one. A stale
`harnex_version` in a fixture, a template or a shipped example fails the suite.
This repository's own `harness.toml` is loaded by no test, so a stale pin
there is silent until `harnex check` reads it, which is what the audit job
runs. Run both; neither alone covers every pin.

Then: bump, commit, tag, push, and watch CI to completion. `gh run watch
--exit-status` is the form that fails when the run does.

The tag names the bump commit. A finding answered after the bump is answered
before the tag, so its commit goes in front of the bump rather than behind it —
`oracle_version` is what a baseline keys a method change on, and a tag standing
somewhere other than the version it names leaves nothing to say which tree that
version was measured from.

## 4 — Prove it, do not assume it

The release is not the artifacts; it is what a machine gets from them.
`release_install_sync` holds the workflow's targets and the installer's asset
names to one set, so the assets and the installer agree by construction — what
it cannot say is that the published binary runs. Install from the release and
run a command that exercises what changed.

For a plugin change, update the installed plugin and read the changed file out
of the install path. The plugin is SHA-driven and moves independently of the
binary version.
