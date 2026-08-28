//! Drift guard between `templates/scaffold.toml`, the template tree, and the
//! language set.
//!
//! The manifest is the only statement of what a generated harness contains,
//! so three relations must hold in both directions: every artifact it names
//! exists on disk for every supported language, every language the permission
//! registry offers has the templates the manifest expects of it, and no
//! per-language template sits in the tree unclaimed. A one-directional check
//! catches the typo and misses the omission — which is the failure that
//! matters, because an omitted artifact scaffolds a harness with a hole in it
//! and nothing downstream can tell.

use std::collections::BTreeSet;
use std::path::PathBuf;

use harness_core::policy::PermissionProfile;
use harness_core::scaffold::{Content, ScaffoldManifest, Tier};

fn templates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/templates")
}

fn manifest() -> ScaffoldManifest {
    ScaffoldManifest::load(&templates_root()).expect("scaffold.toml loads")
}

fn languages() -> Vec<&'static str> {
    PermissionProfile::languages().collect()
}

#[test]
fn the_language_set_is_not_empty() {
    assert!(
        !languages().is_empty(),
        "no `<lang>-dev` profile exists, so every language-tier assertion below would pass vacuously"
    );
}

#[test]
fn every_manifest_template_exists_for_every_language() {
    let root = templates_root();
    let m = manifest();
    for artifact in m.tier(Tier::Foundation) {
        let template = artifact
            .template_for(None)
            .expect("foundation needs no lang");
        assert!(
            root.join(&template).is_file(),
            "scaffold.toml names foundation template '{template}', which is not in the tree"
        );
    }
    for lang in languages() {
        for artifact in m.tier(Tier::Language) {
            let template = artifact
                .template_for(Some(lang))
                .expect("language tier resolves with a language");
            assert!(
                root.join(&template).is_file(),
                "scaffold.toml names '{}' but language '{lang}' has no template at '{template}'",
                artifact.template
            );
        }
    }
}

/// Templates a mode other than `scaffold` installs, so the unclaimed sweep
/// does not read them as dead weight. Each names the verb that emits it.
const NON_SCAFFOLD_TEMPLATES: &[(&str, &str)] = &[
    ("common/rule-template.md", "extend rule"),
    ("common/skill-template.md", "extend skill"),
];

#[test]
fn no_template_is_unclaimed_by_the_manifest() {
    // Both tiers, every language, and `common/` — the earlier sweep walked
    // only the language directories, so three `common/` templates sat
    // unclaimed and unnoticed. A template nothing installs is dead weight or
    // a missing manifest row; one another mode installs is declared above.
    let root = templates_root();
    let m = manifest();
    let mut claimed: BTreeSet<PathBuf> = m
        .tier(Tier::Foundation)
        .filter_map(|a| a.template_for(None))
        .map(|t| root.join(t))
        .collect();
    for lang in languages() {
        claimed.extend(
            m.tier(Tier::Language)
                .filter_map(|a| a.template_for(Some(lang)))
                .map(|t| root.join(t)),
        );
    }
    claimed.extend(NON_SCAFFOLD_TEMPLATES.iter().map(|(rel, _)| root.join(rel)));
    // `patterns/` is its own manifest with its own drift test.
    let unclaimed: Vec<PathBuf> = walk(&root)
        .into_iter()
        .filter(|p| !claimed.contains(p))
        .filter(|p| !p.starts_with(root.join("patterns")))
        .filter(|p| p.file_name().is_some_and(|n| n != "scaffold.toml"))
        .collect();
    assert!(
        unclaimed.is_empty(),
        "templates/ holds files no scaffold.toml artifact emits and no mode declares: {unclaimed:?}"
    );
}

#[test]
fn every_non_scaffold_template_is_a_real_file_a_real_verb_installs() {
    // Both halves, because the allowance is the one place a template can be
    // excused from the sweep. A dead file excused by it is dead weight the
    // sweep exists to find; a verb the skill does not offer excuses a template
    // nothing can install, which is how `pre-push` shipped unreachable behind
    // an `extend pattern pre-push` that was never a verb.
    let root = templates_root();
    let menu = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/harnex/SKILL.md"),
    )
    .expect("SKILL.md is readable");

    for (rel, verb) in NON_SCAFFOLD_TEMPLATES {
        assert!(
            root.join(rel).is_file(),
            "'{rel}' is declared as installed by `{verb}` but is not in the tree"
        );
        assert!(
            menu.contains(&format!("`{verb}")),
            "'{rel}' is excused as installed by `{verb}`, which SKILL.md offers nowhere"
        );
    }
}

/// The manifest's `executable` flag is the only source of a landed hook's
/// mode, so no template carries the bit itself.
///
/// Two candidate owners is one too many. A template that happens to be 0o755
/// makes a consumer copying with `cp -p` land a runnable hook the manifest
/// never marked, and the same reader then reads a 0o644 sibling as a mistake.
/// The failure this prevents is not cosmetic: harnex's hooks are wired in exec
/// form, so the runtime spawns the wrapper itself and a missing bit is EACCES
/// before the script starts — every hook in the harness dies at once, which is
/// what `audit-hook-not-executable` reports after the fact.
#[cfg(unix)]
#[test]
fn no_template_carries_its_own_exec_bit() {
    use std::os::unix::fs::PermissionsExt;

    let root = templates_root();
    let wearing: Vec<String> = walk(&root)
        .into_iter()
        .filter(|p| std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0))
        .map(|p| p.strip_prefix(&root).unwrap().display().to_string())
        .collect();
    assert!(
        wearing.is_empty(),
        "{wearing:?} carry an exec bit; scaffold.toml's `executable` decides the landed mode"
    );
}

#[test]
fn no_two_merge_fragments_claim_the_same_contribution() {
    // Merging is a union — of keys for an object fragment, of elements for an
    // array — so two fragments claiming the same contribution would leave one
    // silently absorbing the other. `scaffold.toml` has two artifacts
    // contributing under `hooks` and two to `permissions.allow`, one per tier.
    //
    // Contributions are compared by their FULL dotted path, so `hooks` carrying
    // a `PostToolUse` object key and `hooks.PostToolUse` carrying elements are
    // recognised as the same JSON location reached two ways. That pair is legal
    // — both contribute an array there and the union merges them — which is why
    // the second assertion below is about SHAPE: two fragments meeting at one
    // location with different shapes fall through the union's object and array
    // arms to replacement, and the first one written disappears.
    let root = templates_root();
    let m = manifest();
    for lang in languages() {
        let mut claimed: BTreeSet<String> = BTreeSet::new();
        let mut shape_at: std::collections::BTreeMap<String, &'static str> =
            std::collections::BTreeMap::new();
        let mut expect_shape = |location: String, shape: &'static str| {
            if let Some(prior) = shape_at.insert(location.clone(), shape) {
                assert_eq!(
                    prior, shape,
                    "two scaffold.toml fragments contribute different shapes at '{location}' \
                     for language '{lang}'; the union replaces rather than merges across shapes, \
                     so whichever lands first is lost"
                );
            }
        };
        for artifact in m.artifacts() {
            let Content::Merge { key: merge } = &artifact.content else {
                continue;
            };
            let template = artifact
                .template_for(Some(lang))
                .expect("template resolves with a language");
            let raw = std::fs::read_to_string(root.join(&template)).unwrap();
            let value: serde_json::Value = serde_json::from_str(&raw)
                .unwrap_or_else(|e| panic!("{template} is not JSON: {e}"));
            match value.as_object() {
                // An object fragment contributes keys; two artifacts may share
                // the destination key as long as no single key is claimed twice.
                Some(object) => {
                    for (key, nested) in object {
                        expect_shape(format!("{merge}.{key}"), shape_of(nested));
                        assert!(
                            claimed.insert(format!("{merge}.{key}")),
                            "two scaffold.toml fragments both contribute '{merge}.{key}' for \
                             language '{lang}'; one grant with two owners is a duplicated \
                             declaration even where the union merges it cleanly"
                        );
                    }
                }
                // An array fragment — every permission list is one —
                // contributes its elements. Two fragments naming the same rule
                // would collapse into one on union and leave that grant with
                // two owners. Any other scalar replaces its slot outright, so
                // the path may be claimed only once.
                None => match value.as_array() {
                    Some(elements) => {
                        expect_shape(merge.clone(), "array");
                        for element in elements {
                            let rule = element.to_string();
                            assert!(
                                claimed.insert(format!("{merge}[{rule}]")),
                                "two scaffold.toml fragments both contribute {rule} to '{merge}' \
                                 for language '{lang}'; the union would hide the duplication"
                            );
                        }
                    }
                    None => {
                        assert!(
                            claimed.insert(merge.clone()),
                            "two scaffold.toml fragments both merge a scalar into '{merge}' for \
                             language '{lang}'; the second would replace the first outright"
                        );
                    }
                },
            }
        }
    }
}

#[test]
fn every_destination_is_claimed_once_per_language() {
    let m = manifest();
    for lang in languages() {
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for artifact in m.artifacts() {
            // A JSON fragment contributes to a shared destination by design;
            // exactly one artifact may own a destination outright.
            if matches!(artifact.content, Content::Merge { .. }) {
                continue;
            }
            let dest = artifact
                .destination_for(Some(lang))
                .expect("destination resolves with a language")
                .to_string_lossy()
                .to_string();
            assert!(
                seen.insert(dest.clone()),
                "two scaffold.toml artifacts both copy to '{dest}' for language '{lang}'; the \
                 second would silently overwrite the first"
            );
        }
    }
}

#[test]
fn the_foundation_tier_stands_alone() {
    // A foundation-only scaffold is what an unsupported stack receives, so
    // nothing in that tier may depend on a language-tier artifact having
    // landed. Every hook the foundation wires must be a foundation artifact.
    let root = templates_root();
    let m = manifest();
    let foundation_destinations: BTreeSet<String> = m
        .tier(Tier::Foundation)
        .filter_map(|a| a.destination_for(None))
        .map(|d| d.to_string_lossy().to_string())
        .collect();

    for artifact in m.tier(Tier::Foundation) {
        if !matches!(artifact.content, Content::Merge { .. }) {
            continue;
        }
        let template = artifact.template_for(None).unwrap();
        let raw = std::fs::read_to_string(root.join(&template)).unwrap();
        // Hook fragments are objects; a permission list is an array and its
        // rules are patterns rather than paths. Scanning those for anchored
        // paths would fail this test on the first permission rule that
        // legitimately mentions one.
        let Ok(fragment) = serde_json::from_str::<serde_json::Value>(&raw) else {
            panic!("fragment '{template}' is not JSON");
        };
        if !fragment.is_object() {
            continue;
        }
        for referenced in wired_destinations(&fragment) {
            assert!(
                foundation_destinations.contains(&referenced),
                "foundation hook fragment '{template}' wires '{referenced}', which the foundation \
                 tier does not emit — a foundation-only scaffold would wire a handler to nothing"
            );
        }
    }
}

/// Every project path a hook fragment wires: the wrapper its `command` names,
/// and the verifier that wrapper dispatches.
///
/// The second half is where this invariant actually lives. harnex's wrappers
/// take a verifier's bare name as `args[0]` and resolve it under `hooks/`, and
/// every fragment in both tiers names the same two wrappers — so `command`
/// alone can never tell a foundation fragment from one reaching into the
/// language tier, and a guard reading only `command` passes vacuously.
///
/// The auditor deliberately refuses this resolution, because what an arbitrary
/// project's wrapper does with its arguments is not knowable from outside it.
/// Here it is knowable: these are harnex's own templates and the convention is
/// harnex's own.
fn wired_destinations(fragment: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(events) = fragment.as_object() else {
        return out;
    };
    for entries in events.values() {
        for entry in entries.as_array().into_iter().flatten() {
            let handlers = entry.get("hooks").and_then(|h| h.as_array());
            for handler in handlers.into_iter().flatten() {
                let command = handler
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default();
                // Keyed on the key's presence, exactly as the auditor is:
                // `"args": []` is still a direct spawn, and asking whether the
                // list is empty instead would send one fragment down the shell
                // grammar here and the exec grammar there — two readers again.
                let Some(args) = handler.get("args").and_then(|a| a.as_array()) else {
                    out.extend(harness_core::guard::paths_in_command(command));
                    continue;
                };
                let args: Vec<&str> = args.iter().filter_map(|v| v.as_str()).collect();

                out.extend(harness_core::guard::path_in_argument(command));
                for (i, arg) in args.iter().enumerate() {
                    match harness_core::guard::path_in_argument(arg) {
                        Some(path) => {
                            out.insert(path);
                        }
                        // A bare first argument is the verifier name the
                        // wrapper resolves under `hooks/`. A flag is not a
                        // name: `hooks/--verbose` would fail the tier
                        // assertion on a legitimate fragment.
                        None if i == 0 && !arg.starts_with('-') => {
                            out.insert(format!("hooks/{arg}"));
                        }
                        None => {}
                    }
                }
            }
        }
    }
    out
}

fn walk(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// The JSON shape a fragment contributes at a location. Two fragments meeting
/// at one location must agree, because the union merges objects into objects
/// and arrays into arrays and replaces across the two.
fn shape_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Object(_) => "object",
        serde_json::Value::Array(_) => "array",
        _ => "scalar",
    }
}

/// What a baseline treats as the harness has to reach everywhere a harness is
/// written, or a project scaffolded by this plugin has part of its harness
/// invisible to the comparison that asks whether changing it did anything.
#[test]
fn every_scaffolded_artifact_is_inside_the_default_harness() {
    let harness = harness_core::config::default_harness_paths();
    let m = manifest();

    for artifact in m.artifacts() {
        for language in languages().into_iter().map(Some).chain([None]) {
            let Some(destination) = artifact.destination_for(language) else {
                continue;
            };
            let destination = destination.to_string_lossy().to_string();
            assert!(
                harness.iter().any(
                    |root| destination == *root || destination.starts_with(&format!("{root}/"))
                ),
                "`{destination}` is scaffolded and no default harness path reaches it"
            );
        }
    }
}
