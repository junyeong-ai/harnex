---
paths:
  - "crates/harness-core/src/spec.rs"
  - "crates/harness-core/src/validate/**"
  - "crates/harness-core/tests/spec_facts_sync.rs"
  - "plugins/harnex/reference/spec-facts.md"
governs:
  concept: measurement stamps for the Claude Code vocabularies
  live_truth:
    - crates/harness-core/src/spec.rs
---

# spec — measurement stamps for the Claude Code vocabularies

Every `KNOWN_*` set mirrors a surface of the Claude Code spec. The tests that
guard them compare each set to the plugin's reference doc — a closed loop,
because both sides are ours. `SpecSurface` closes it against the calendar:
each surface records the date its vocabulary was read from its page and a
digest of what was read.

The digest is what makes the date honest. `spec_stamps_match_live_vocabularies`
holds `digest` equal to the live constants, so editing a set without
re-reading the page fails the build — a stamp can never describe a vocabulary
it no longer covers.

Each validator declares its own sets in one `SPEC_SETS` beside its constants,
and the digest covers label-plus-values per set — so a value moving between two
sets, or a set being renamed, moves the digest.

That a *new* constant reaches that list is **discipline-held**, and the reason
is a trade rather than an absence of options. Scanning this crate's source is
forbidden by `keep-soften-cut`, and `DANGEROUS_ALLOW_BASES` is a live instance
of the false positive such a scan would produce. Deriving each constant from
its `SPEC_SETS` row does work and costs nothing at runtime — but it puts every
vocabulary one hop from its values, and reading these constants against a
documentation page is the whole activity they exist for. Taxing that to defend
a registration row is the wrong way round, and an author can still declare a
bare constant regardless. The declaration therefore buys visibility, not
enforcement.

## Adding or changing a vocabulary

1. Re-read the surface's `doc` page against the live documentation.
2. Edit the constant.
3. Set `measured` to today and `digest` to the value the failing test prints.

Steps 1 and 3 are the same act: the test refuses to pass until the stamp moves,
and moving the stamp without step 1 is the one failure no mechanism here can
catch.

## Where staleness surfaces

`spec::stale_warnings_now()` on the envelope's `warnings[]`, attached by
`write_envelope_success` to every command. Not a finding, and never gating:
staleness is a property of this binary rather than of the project a command
was pointed at, and an old answer is unverified, not known wrong.

## Refusals

No fetching (Article I keeps the network out of command time — a scheduled job
may fetch and open a pull request), and no re-deriving a set by parsing a
rendered page: extraction from prose has a false-positive floor, and a wrong
auto-update is worse than a stale one that says so.

`digest` is a hand-rolled FNV-1a because the value is committed to source.
`DefaultHasher` is explicitly unstable across releases, so a toolchain bump
would rewrite every stamp and the guard would read as drift.
