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

A release that moves what a gate reports about unmodified input is deciding
this, whether or not one of those surfaces moved. Direction is the whole
question: a true finding newly reported is the gate catching up, and a true
finding that stops being reported — or a false one that starts — is the break.
Answer it by running the gate over a corpus before and after and diffing the
findings, and answer it over a corpus that can produce the change. 0.8.1
shipped a silent false pass because its corpus could not: six of its 13,711
citations reached the changed code path.

## 3 — Run the chain

Run the gates so **their exit status is the run's**. This has failed once: a
wrapper captured a gate in a command substitution, the suite went red, and the
substitution returned its output rather than its status — the chain continued
over a failure the suite had already found.

Every CI job needs a local counterpart before a tag, and
`.github/workflows/ci.yml` is the list to check that against. Two kinds of gap,
and only one of them is a hole. A job whose twin was never written is a hole:
v0.6.0 was tagged over a schema drift for exactly that, and `schema_sync.rs` is
the twin that was missing. A job that cannot run here is not a hole: the
test matrix carries two operating systems and a development machine is one of
them, so the other leg is only ever green in CI. That is why the run is watched
rather than predicted.

The pins are not a manual sweep, but they take two gates and not one. A stale
`harnex_version` in a fixture, a template or a shipped example fails the suite —
eight targets, measured. This repository's own `harness.toml` is loaded by no
test, so a stale pin there is silent until `harnex check` reads it, which is
what the audit job runs. Run both; neither alone covers all eight sites.

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
