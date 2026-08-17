---
paths:
  - "plugins/harnex/templates/scaffold.toml"
  - "crates/harness-core/src/scaffold.rs"
  - "crates/harness-core/tests/scaffold_manifest.rs"
  - "crates/harness-core/tests/plugin_scaffold_validates.rs"
  - "crates/harness-core/tests/adopted_scaffold_matches_templates.rs"
---

# scaffold — the composition manifest

`plugins/harnex/templates/scaffold.toml` declares every template-derived
artifact a generated harness contains. Emissions whose content comes from the
project rather than from a template — `rustfmt.toml`'s detected edition, a
cloud permission profile — belong to the skill, which names them beside its
reference to this file. `ScaffoldManifest::load` parses and validates it; nothing in
Rust names a template or destination path (Constitution VII).

Three consumers, one declaration:
- the skill emits the artifacts,
- `tests/plugin_scaffold_validates.rs` builds its fixture from them,
- `ProjectAuditor` reports coverage and drives the managed-region auditor.

A file list written a second time is the one that drifts — the fixture omitted
every hook while reporting a clean scaffold before this manifest existed.

## Tiers

`Tier::Foundation` carries no language dependency and is what a stack with no
profile still receives. `Tier::Language` needs a detected stack. The split is
what makes a partial harness expressible instead of an all-or-nothing refusal.

## Content kinds

`content.kind` (`crates/harness-core/src/scaffold.rs:99`) decides three answers
that must agree — how the artifact is emitted, how its presence is tested, what
counts as drift — so it is one field rather than three flags. Independent booleans could also spell "merged and
managed", a state that named nothing and had to be rejected at load.

| kind | project copy | presence is | drift is |
|---|---|---|---|
| `copy` | byte-identical to the template | destination exists | a defect |
| `seed` | the project's, from first write | destination exists | not asserted |
| `managed` | sentinels bound harnex's region | destination exists | edits inside sentinels |
| `merge` | one fragment among several | the fragment landed at `key` | not asserted |

`merge` is the only kind whose destination is shared, which is why presence is
containment of its fragment rather than the file existing: a foundation-only
scaffold otherwise reports its unmerged language rows present, because the
foundation tier wrote the file they name. `scaffold::fragment_landed` is that
predicate, and both the auditor's coverage check and the self-dogfood test
read it — a second containment rule would be the one that drifts.

The union is by fragment shape: an object contributes its keys, an array its
elements as a sorted set. `hooks` and `permissions.allow` are each claimed by
both tiers, and a replacing merge would erase the foundation's Stop hook, or
its `Edit`/`Write` grants, the moment the language fragment landed —
validating clean while running nothing.

`seed` exists because a project's governance is its own. Holding it to the
template would make tailoring — the intended use — read as drift.

## Operability

A scaffold must be runnable, not merely well-formed. `harness.toml` is a
foundation artifact for that reason: without it the generated `governance.md`
sends its reader to `harness lifecycle observe|candidates|retire` and
`harness telemetry report`, and every one answers CONFIG_NOT_FOUND. The
`workspace` allow floor is one for the same reason a step lower — every command
those rules name prompts without it. And `harness-curate` is the skill that
runs the sweep, because a procedure over several commands is what invariant 2
assigns to a skill, and a rule can carry neither `allowed-tools` nor a trigger
that fires when someone sits down to run it.

`assert_scaffold_is_operable` holds the emitted fixture to the config surfaces,
and the skill sweep asserts the emitted set is non-empty — auditing artifacts
can never catch a missing artifact that was the one making the rest reachable.

The fixture reads every policy from the scaffolded `harness.toml`, never from a
literal. A restated policy is one no real project has: the fixture would pass
under settings the scaffold does not ship, which is how an always-loaded rule
and a strict skill policy both went green here while failing in the field.

## Adding an artifact

1. Add the `[[artifact]]` block with its `content.kind`. `executable` = written
   0o755.
2. Nothing else changes for a per-language artifact: `{lang}` resolves against
   the `<lang>-dev` members of `PermissionProfile::ALL`.
3. `tests/scaffold_manifest.rs` holds the relations both ways — every named
   template exists for every language, every per-language template is claimed,
   every non-merge destination is claimed once, and the foundation tier wires
   only foundation artifacts.

Load-time validation rejects: a destination that is absolute, tilde-rooted, or
escapes with `..`; a template escaping the templates directory; `{lang}` in a
foundation artifact (the tier must resolve with no language); a `managed`
artifact on the language tier; an empty merge key path; a merge into a non-JSON
destination; and any field outside the declared shape (Constitution V — a
misspelled field would otherwise leave an artifact silently weaker).

`{lang}` resolves only against an identifier (`[a-z0-9-]+`). Containment is
checked at load time against the unsubstituted string, so a language carrying
a separator would rewrite the shape that check approved; both resolvers are
public, and a guarantee that holds only while every caller behaves is not one.
