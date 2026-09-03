---
description: "Take a change from converged review to a proven release: two independent whole-surface reviews, the version decision, the gate run, tag, push, CI, assets, and a proof against the released binary. Reads the workspace gates, `.github/workflows/ci.yml`, and `gh run`; writes the version bump, the tag, and the release."
when_to_use: "At a milestone — work is complete and about to leave this repository. Not for an ordinary commit, which needs the gates and nothing here. Manual only: it tags and pushes, and neither is undone by re-running."
disable-model-invocation: true
allowed-tools: Read Edit Bash(cargo *) Bash(harnex *) Bash(git *) Bash(gh *) Bash(claude plugin *)
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
- **Mutation-test every guard the change adds or touches.** Remove what the
  guard is supposed to catch, confirm it fails, restore. A guard that passes
  its own mutation is watching nothing, and the suite still reports green.

Do not start §3 while a claim is open. An unresolved finding after a tag is a
finding that ships.

## 2 — Decide the version

`making-changes.md` owns the rule: the minor is how a break in a contract
outside this repository is announced, and it ships in the same release as the
break. Decide which this is before bumping, and say why in the release commit.

`[meta] harnex_version` pins a range, so a break shipped inside the pinned
range makes the gate state a compatibility that does not hold.

## 3 — Run the chain

Run the gates so **their exit status is the run's**. This has failed once: a
wrapper captured a gate in a command substitution, the suite went red, and the
substitution returned its output rather than its status — the chain continued
over a failure the suite had already found.

Every CI job has a local twin, and `.github/workflows/ci.yml` is the list of
jobs that must pass. This has failed once too: v0.6.0 was tagged before CI
found a schema drift that no local guard held, because a job existed with no
local counterpart. Before tagging, check that list against what runs here; a
job with no twin is the next one to find something after a tag.

The pins are not a manual sweep. A stale `harnex_version` fails the suite in
eight targets, the shipped examples among them — a red suite is the answer,
not a grep.

Then: bump, commit, tag, push, and watch CI to completion. `gh run watch
--exit-status` is the form that fails when the run does.

## 4 — Prove it, do not assume it

The release is not the artifacts; it is what a machine gets from them.
`release_install_sync` holds the workflow's targets and the installer's asset
names to one set, so the assets and the installer agree by construction — what
it cannot say is that the published binary runs. Install from the release and
run a command that exercises what changed.

For a plugin change, update the installed plugin and read the changed file out
of the install path. The plugin is SHA-driven and moves independently of the
binary version.
