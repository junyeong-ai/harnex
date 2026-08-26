//! Built-in permission profiles.
//!
//! Each profile contributes `deny`, `ask`, and `allow` rule lists. The
//! `baseline` profile captures truly OS-universal hazards (secrets read,
//! `sudo`, `rm -rf $HOME`); ecosystem profiles (git, gcp, aws) opt-in.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct PermissionProfile {
    pub name: &'static str,
    pub allow: Vec<&'static str>,
    pub ask: Vec<&'static str>,
    pub deny: Vec<&'static str>,
}

impl PermissionProfile {
    pub const ALL: &'static [&'static str] = &[
        "baseline",
        "workspace",
        "git-strict",
        "gcp-strict",
        "aws-strict",
        "rust-dev",
        "python-dev",
        "typescript-dev",
        "jvm-dev",
    ];

    /// Languages the oracle ships a dev profile for — what the scaffold
    /// manifest's `{lang}` resolves against. Derived from [`Self::ALL`], so
    /// adding a `<lang>-dev` profile widens every language-tier surface at
    /// once and no second list can disagree with this one.
    pub fn languages() -> impl Iterator<Item = &'static str> {
        Self::ALL.iter().filter_map(|n| n.strip_suffix("-dev"))
    }

    pub fn from_str(name: &str) -> Option<Self> {
        Some(match name {
            "baseline" => baseline(),
            "workspace" => workspace(),
            "git-strict" => git_strict(),
            "gcp-strict" => gcp_strict(),
            "aws-strict" => aws_strict(),
            "rust-dev" => rust_dev(),
            "python-dev" => python_dev(),
            "typescript-dev" => typescript_dev(),
            "jvm-dev" => jvm_dev(),
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        self.name
    }
}

/// OS-universal hazards: secrets access, arbitrary code execution,
/// destructive git, filesystem destruction. Every project should include
/// this profile.
///
/// Rule grammar follows the Claude Code permission spec: Bash uses the
/// canonical space-then-`*` wildcard (`Bash(cmd *)`); Read/Edit use
/// gitignore-style globs where a bare pattern matches at any depth
/// (`Read(.env)` ≡ `Read(**/.env)`). Which rules a permission check actually
/// reads is [`super::rule`]'s to say, and `every_profile_rule_is_consulted`
/// holds every rule below to it. Two redundancy rules the spec lets us
/// drop: Read deny rules already cover `cat`/`head`/`tail`/`sed` of the same
/// path, so no `Bash(cat .env *)` mirror is needed; and built-in read-only
/// commands (`ls`, `grep`, `find`, read-only `git`, …) never prompt, so they
/// never appear in an allow list.
fn baseline() -> PermissionProfile {
    PermissionProfile {
        name: "baseline",
        allow: vec![],
        ask: vec![],
        deny: vec![
            // --- sensitive file reads (exfiltration guard; Read deny also
            // blocks cat/head/tail/sed and neutralises Edit, which must read
            // first). Patterns are precise file SHAPES — extensions, the
            // `secrets/` dir, credential JSON, home credential paths — never a
            // broad substring like `*secret*`, which would hard-block source
            // files such as `secret_manager.ts` or `secrets.service.ts`. ---
            "Read(.env)",
            "Read(.env.*)",
            "Read(*.pem)",
            "Read(*.key)",
            "Read(*.p12)",
            "Read(*.pfx)",
            "Read(*credentials*.json)",
            "Read(/secrets/**)",
            "Read(~/.ssh/*)",
            "Read(~/.aws/credentials)",
            // --- sensitive file edits (corruption guard). `Edit` is the
            // whole write floor; [`super::rule`] holds why a `Write(path)`
            // twin beside it would be inert.
            //
            // Shapes are PRECISE deployment names — not broad `.env.*` —
            // because `deny > allow` makes broad denies unoverridable, so a
            // blanket `Edit(.env.*)` would block legitimate scaffolding of
            // `.env.example` / `.env.sample` / `.env.template` with no
            // project-level escape hatch. The read denies above are broader
            // because exfiltration is the concern there, and reading
            // `.env.example` is unlikely to be programmatic.
            //
            // Deployment-env deny shapes cover the common naming conventions;
            // projects with additional deployment env names add them via
            // `[policy.permissions].extra_deny`. ---
            "Edit(.env)",
            "Edit(.env.local)",
            "Edit(.env.development)",
            "Edit(.env.staging)",
            "Edit(.env.production)",
            "Edit(*.pem)",
            "Edit(*.key)",
            "Edit(*.p12)",
            "Edit(*.pfx)",
            "Edit(/secrets/**)",
            "Edit(~/.ssh/*)",
            "Edit(~/.aws/credentials)",
            // --- destructive git ---
            "Bash(git push --force *)",
            "Bash(git push -f *)",
            "Bash(git reset --hard *)",
            "Bash(git checkout .)",
            "Bash(git checkout -- .)",
            "Bash(git restore .)",
            "Bash(git restore -- .)",
            "Bash(git branch -D main)",
            "Bash(git branch -D master)",
            "Bash(git clean -fd *)",
            "Bash(git clean -fdx *)",
            "Bash(git rebase -i *)",
            // Blanket staging, enumerated rather than approximated: `git add`
            // has exactly these spellings for "everything", and a deny that
            // covers three of five leaves `workspace`'s `Bash(git add *)`
            // approving the other two. Enumerating a documented flag set is
            // exhaustive, not a guess at what someone might type.
            "Bash(git add .)",
            "Bash(git add -A *)",
            "Bash(git add --all *)",
            "Bash(git add -u *)",
            "Bash(git add --update *)",
            "Bash(git add :/ *)",
            // Irreversible stash subcommands. `git stash` itself is a save and
            // stays allowed; these two discard work with no reflog to recover
            // it, which is the property that earns a deny.
            "Bash(git stash clear *)",
            "Bash(git stash drop *)",
            // --- filesystem destruction ---
            "Bash(rm -rf /)",
            "Bash(rm -rf /*)",
            "Bash(rm -rf ~)",
            "Bash(rm -rf ~/*)",
            "Bash(rm -rf $HOME)",
            "Bash(rm -rf $HOME/*)",
            "Bash(rm -rf .git*)",
            "Bash(chmod -R 777 *)",
            "Bash(sudo *)",
            // --- arbitrary code execution (escapes every rule above) ---
            "Bash(node -e *)",
            "Bash(node --eval *)",
            "Bash(python -c *)",
            "Bash(python3 -c *)",
            // --- destructive find forms (safe forms stay built-in read-only) ---
            "Bash(find * -exec *)",
            "Bash(find * -delete)",
        ],
    }
}

fn git_strict() -> PermissionProfile {
    PermissionProfile {
        name: "git-strict",
        allow: vec![],
        ask: vec!["Bash(git push *)", "Bash(git rebase *)"],
        deny: vec![
            "Bash(git push --force *)",
            "Bash(git push -f *)",
            "Bash(git push --force-with-lease *)",
            "Bash(git reset --hard *)",
            "Bash(git checkout .)",
            "Bash(git checkout -- .)",
            "Bash(git restore .)",
            "Bash(git restore -- .)",
            "Bash(git branch -D main)",
            "Bash(git branch -D master)",
            "Bash(git branch -D prd)",
            "Bash(git branch -D production)",
            "Bash(git rebase -i *)",
            "Bash(git rebase --interactive *)",
            "Bash(git clean -fd *)",
            "Bash(git clean -fdx *)",
        ],
    }
}

/// GCP destruction patterns: project/IAM/KMS/Run/SQL/Secrets deletion,
/// storage removal, IAM policy mutation, plus IaC and k8s destructors.
fn gcp_strict() -> PermissionProfile {
    PermissionProfile {
        name: "gcp-strict",
        allow: vec![],
        ask: vec!["Bash(gcloud * deploy *)", "Bash(gcloud * apply *)"],
        deny: vec![
            "Bash(gcloud projects delete *)",
            "Bash(gcloud * projects delete *)",
            "Bash(gcloud organizations *)",
            "Bash(gcloud * organizations *)",
            "Bash(gcloud iam roles delete *)",
            "Bash(gcloud * iam roles delete *)",
            "Bash(gcloud iam service-accounts delete *)",
            "Bash(gcloud * iam service-accounts delete *)",
            "Bash(gcloud iam service-accounts keys create *)",
            "Bash(gcloud * iam service-accounts keys create *)",
            "Bash(gcloud kms keys destroy *)",
            "Bash(gcloud * kms keys destroy *)",
            "Bash(gcloud run services delete *)",
            "Bash(gcloud * run services delete *)",
            "Bash(gcloud run jobs delete *)",
            "Bash(gcloud * run jobs delete *)",
            "Bash(gcloud sql instances delete *)",
            "Bash(gcloud * sql instances delete *)",
            "Bash(gcloud secrets delete *)",
            "Bash(gcloud * secrets delete *)",
            "Bash(gcloud storage rm *)",
            "Bash(gcloud * storage rm *)",
            "Bash(gsutil rm *)",
            "Bash(gsutil -m rm *)",
            // --- IAM policy mutation ---
            "Bash(gcloud * set-iam-policy *)",
            "Bash(gcloud * remove-iam-policy-binding *)",
            // --- IaC destruction ---
            "Bash(terraform destroy *)",
            "Bash(terraform state rm *)",
            // --- k8s destructors ---
            "Bash(kubectl delete *)",
        ],
    }
}

fn aws_strict() -> PermissionProfile {
    PermissionProfile {
        name: "aws-strict",
        allow: vec![],
        ask: vec!["Bash(aws * delete *)", "Bash(aws * update *)"],
        deny: vec![
            "Bash(aws iam delete-* *)",
            "Bash(aws s3 rb *)",
            "Bash(aws s3 rm * --recursive)",
            "Bash(aws rds delete-db-instance *)",
            "Bash(aws ec2 terminate-instances *)",
            "Bash(aws kms schedule-key-deletion *)",
            "Bash(aws lambda delete-function *)",
            "Bash(aws cloudformation delete-stack *)",
        ],
    }
}

/// The allow floor for working in a repository at all — the counterpart to
/// `baseline`'s deny floor, and carrying no language dependency, so it is what
/// a stack with no `<lang>-dev` profile still receives.
///
/// Only commands that actually prompt are listed: `Edit`/`Write` (file
/// modification requires approval) and the mutating filesystem/git commands.
/// Read-only built-ins (`ls`, `grep`, `find`, `cat`, `diff`, `wc`, read-only
/// `git status`/`diff`/`log`/`show`, …) are omitted — Claude Code runs them
/// without a prompt in every mode, so an allow rule would be a no-op. The
/// destructive git forms denied by `baseline` still win under deny > allow.
///
/// `cp`/`mv` are deliberately NOT pre-approved. A `Read(.env)`-class deny only
/// reaches `cat`/`head`/`tail`/`sed` (per the spec), so `cp .env /tmp/x` then
/// reading the copy would slip a secret past the deny. Leaving cp/mv to prompt
/// keeps that move visible to the operator; legitimate moves still succeed on
/// approval. `mkdir -p` stays — creating a directory cannot exfiltrate.
///
/// `harness` is here because a generated harness ships `harness.toml` and
/// rules that send their reader to `harness lifecycle` / `harness telemetry` /
/// `harness check`. A documented loop whose every step raises a prompt is a
/// loop nobody runs twice.
fn workspace() -> PermissionProfile {
    PermissionProfile {
        name: "workspace",
        allow: vec![
            "Edit",
            "Write",
            "Bash(mkdir -p *)",
            "Bash(git add *)",
            "Bash(git commit *)",
            "Bash(git branch *)",
            "Bash(git stash *)",
            "Bash(git checkout -b *)",
            "Bash(git switch *)",
            "Bash(harness *)",
        ],
        ask: vec![],
        deny: vec![],
    }
}

/// Rust development toolchain: cargo (covers `cargo clippy`/`cargo fmt`)
/// plus standalone rustfmt.
fn rust_dev() -> PermissionProfile {
    PermissionProfile {
        name: "rust-dev",
        allow: vec!["Bash(cargo *)", "Bash(rustfmt *)"],
        ask: vec![],
        deny: vec![],
    }
}

/// Python development toolchain: uv, python/python3, pytest, ruff, and the
/// three mainstream type checkers. `python -c` and `python3 -c` stay denied by
/// `baseline`.
///
/// A dev profile is the ecosystem's mainstream toolchain, not a claim about
/// which of it this project uses — an allow for an absent tool never matches,
/// while a missing one prompts on every invocation. Naming only `mypy` here
/// also contradicted the language matrix, which reads `ty`.
fn python_dev() -> PermissionProfile {
    PermissionProfile {
        name: "python-dev",
        allow: vec![
            "Bash(uv *)",
            "Bash(python *)",
            "Bash(python3 *)",
            "Bash(pytest *)",
            "Bash(ruff *)",
            "Bash(ty *)",
            "Bash(mypy *)",
            "Bash(pyright *)",
        ],
        ask: vec![],
        deny: vec![],
    }
}

/// TypeScript development toolchain: pnpm, node, tsx, tsc, biome. `node -e`
/// stays denied by `baseline`. The broad `npx *` is deliberately excluded —
/// env-runners execute arbitrary inner commands, so the spec advises a
/// specific `Bash(npx <tool> *)` rule, which the skill adds per project rather
/// than granting wholesale here.
fn typescript_dev() -> PermissionProfile {
    PermissionProfile {
        name: "typescript-dev",
        allow: vec![
            "Bash(pnpm *)",
            "Bash(node *)",
            "Bash(tsx *)",
            "Bash(tsc *)",
            "Bash(biome *)",
        ],
        ask: vec![],
        deny: vec![],
    }
}

/// JVM development toolchain — one profile for Java and Kotlin, because the
/// surface that prompts is the build tool and both languages drive the same
/// one. Wrapper and installed spellings are both granted (`./gradlew` and
/// `gradle`, `./mvnw` and `mvn`) since a project types whichever it has.
///
/// `Bash(java *)` is deliberately absent. In a Gradle or Maven project the
/// build tool is the entry point for every compile, test, and run, so a bare
/// `java` grant buys almost nothing while reaching `java -jar <anything>` —
/// arbitrary bytecode with no flag-level deny that separates it from a
/// legitimate run. The narrower grant is the honest one; a project that runs
/// artifacts directly adds the rule it actually needs.
fn jvm_dev() -> PermissionProfile {
    PermissionProfile {
        name: "jvm-dev",
        allow: vec![
            "Bash(./gradlew *)",
            "Bash(gradle *)",
            "Bash(./mvnw *)",
            "Bash(mvn *)",
            "Bash(google-java-format *)",
            "Bash(ktlint *)",
        ],
        ask: vec![],
        deny: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{PermissionRule, RuleEffect};

    /// Every `<lang>-dev` profile, derived from the registry rather than
    /// listed. A language added to `ALL` then fails here instead of passing
    /// unexamined.
    fn dev_profile_names() -> impl Iterator<Item = &'static &'static str> {
        PermissionProfile::ALL
            .iter()
            .filter(|n| n.ends_with("-dev"))
    }

    #[test]
    fn known_profiles_round_trip_through_registry() {
        for name in PermissionProfile::ALL {
            let profile = PermissionProfile::from_str(name).expect("profile must parse");
            assert_eq!(profile.as_str(), *name);
        }
        assert!(PermissionProfile::from_str("made-up").is_none());
    }

    #[test]
    fn baseline_deny_list_is_non_empty() {
        let p = baseline();
        assert!(!p.deny.is_empty(), "baseline must have deny patterns");
    }

    #[test]
    fn baseline_includes_destructive_git_denies() {
        let p = baseline();
        assert!(p.deny.contains(&"Bash(git push --force *)"));
        assert!(p.deny.contains(&"Bash(git push -f *)"));
        assert!(p.deny.contains(&"Bash(git reset --hard *)"));
        assert!(p.deny.contains(&"Bash(git checkout .)"));
        // Every spelling of "stage everything", because `workspace` grants
        // `Bash(git add *)` to every scaffolded repo and a partial deny leaves
        // the rest auto-approved.
        for blanket in [
            "Bash(git add .)",
            "Bash(git add -A *)",
            "Bash(git add --all *)",
            "Bash(git add -u *)",
            "Bash(git add --update *)",
            "Bash(git add :/ *)",
        ] {
            assert!(p.deny.contains(&blanket), "missing deny {blanket}");
        }
        assert!(p.deny.contains(&"Bash(git stash clear *)"));
        assert!(p.deny.contains(&"Bash(git stash drop *)"));
        assert!(p.deny.contains(&"Bash(git clean -fd *)"));
        assert!(p.deny.contains(&"Bash(git rebase -i *)"));
        assert!(p.deny.contains(&"Bash(rm -rf .git*)"));
    }

    #[test]
    fn baseline_includes_sensitive_file_denies() {
        let p = baseline();
        // Read (exfiltration) — full precise set
        for r in [
            "Read(.env)",
            "Read(.env.*)",
            "Read(*.pem)",
            "Read(*.key)",
            "Read(*credentials*.json)",
            "Read(/secrets/**)",
            "Read(~/.ssh/*)",
            "Read(~/.aws/credentials)",
        ] {
            assert!(p.deny.contains(&r), "missing deny {r}");
        }
        // Edit (corruption). Shapes are PRECISE deployment names, not broad
        // `.env.*`, because `deny > allow` makes broad denies unoverridable —
        // scaffolding `.env.example` would be blocked with no project-level
        // escape hatch. `*credentials*.json` is intentionally Read-only (mock
        // credential fixtures for tests are a legitimate write target).
        for shape in [
            "Edit(.env)",
            "Edit(.env.local)",
            "Edit(.env.development)",
            "Edit(.env.staging)",
            "Edit(.env.production)",
            "Edit(*.pem)",
            "Edit(*.key)",
            "Edit(*.p12)",
            "Edit(*.pfx)",
            "Edit(/secrets/**)",
            "Edit(~/.ssh/*)",
            "Edit(~/.aws/credentials)",
        ] {
            assert!(p.deny.contains(&shape), "missing deny {shape}");
        }
        // Scaffolding shapes must NOT be in the deny list — agents must be
        // able to create `.env.example` / `.env.sample` / `.env.template`
        // without a project-level override.
        for safe in [
            "Edit(.env.example)",
            "Edit(.env.sample)",
            "Edit(.env.template)",
        ] {
            assert!(
                !p.deny.contains(&safe),
                "scaffolding shape '{safe}' must not be denied by baseline"
            );
        }
    }

    #[test]
    fn profile_allows_never_contradict_baseline_deny() {
        // Safety invariant: no allow may be the exact string of a baseline
        // deny (deny > allow wins regardless, but an exact dup is a config
        // smell the auditor flags). Every profile carrying allows is checked,
        // not just the language ones — `workspace` grants git and filesystem
        // verbs that sit closest to the deny floor.
        let deny: std::collections::HashSet<&str> = baseline().deny.into_iter().collect();
        for name in PermissionProfile::ALL {
            let p = PermissionProfile::from_str(name).unwrap();
            for a in &p.allow {
                assert!(
                    !deny.contains(a),
                    "profile '{name}' allow '{a}' is also a baseline deny"
                );
            }
        }
    }

    #[test]
    fn baseline_omits_redundant_and_false_positive_rules() {
        let p = baseline();
        // Read deny already covers `cat .env`; the bare gitignore form already
        // matches at any depth — neither mirror should exist.
        assert!(!p.deny.iter().any(|d| d.starts_with("Bash(cat ")));
        assert!(!p.deny.contains(&"Read(**/.env)"));
        // Broad substrings hard-block source files (`secret_manager.ts`); the
        // floor uses precise shapes instead and must never carry these.
        assert!(!p.deny.contains(&"Read(*secret*)"));
        assert!(!p.deny.contains(&"Read(*credentials*)"));
    }

    /// Every rule a profile ships must be one Claude Code consults. A rule it
    /// accepts and never reads is worse than a missing one: the profile
    /// promises a floor and the settings file it generates enforces nothing.
    #[test]
    fn every_profile_rule_is_consulted() {
        for name in PermissionProfile::ALL {
            let p = PermissionProfile::from_str(name).unwrap();
            for rule in p.allow.iter().chain(&p.ask).chain(&p.deny) {
                if let RuleEffect::Inert(inert) = PermissionRule::parse(rule).effect() {
                    panic!(
                        "profile '{name}' rule '{rule}' is never consulted — {}; {}",
                        inert.reason_text(),
                        inert.hint()
                    );
                }
            }
        }
    }

    #[test]
    fn baseline_includes_code_execution_denies() {
        let p = baseline();
        assert!(p.deny.contains(&"Bash(python3 -c *)"));
        assert!(p.deny.contains(&"Bash(python -c *)"));
        assert!(p.deny.contains(&"Bash(node -e *)"));
        assert!(p.deny.contains(&"Bash(node --eval *)"));
        assert!(p.deny.contains(&"Bash(find * -exec *)"));
        assert!(p.deny.contains(&"Bash(find * -delete)"));
        assert!(p.deny.contains(&"Bash(sudo *)"));
        assert!(p.deny.contains(&"Bash(chmod -R 777 *)"));
    }

    #[test]
    fn gcp_strict_deny_list_is_non_empty() {
        let p = gcp_strict();
        assert!(!p.deny.is_empty(), "gcp-strict must have deny patterns");
    }

    #[test]
    fn gcp_strict_includes_expanded_denies() {
        let p = gcp_strict();
        assert!(p.deny.contains(&"Bash(gcloud run jobs delete *)"));
        assert!(p.deny.contains(&"Bash(gcloud storage rm *)"));
        assert!(p.deny.contains(&"Bash(gsutil rm *)"));
        assert!(p.deny.contains(&"Bash(gcloud * set-iam-policy *)"));
        assert!(
            p.deny
                .contains(&"Bash(gcloud * remove-iam-policy-binding *)")
        );
        assert!(p.deny.contains(&"Bash(terraform destroy *)"));
        assert!(p.deny.contains(&"Bash(terraform state rm *)"));
        assert!(p.deny.contains(&"Bash(kubectl delete *)"));
    }

    #[test]
    fn every_dev_profile_carries_its_toolchain() {
        for (name, expected) in [
            ("rust-dev", &["Bash(cargo *)", "Bash(rustfmt *)"][..]),
            (
                "python-dev",
                &[
                    "Bash(uv *)",
                    "Bash(python *)",
                    "Bash(python3 *)",
                    "Bash(pytest *)",
                    "Bash(ruff *)",
                    "Bash(ty *)",
                    "Bash(mypy *)",
                    "Bash(pyright *)",
                ][..],
            ),
            (
                "typescript-dev",
                &[
                    "Bash(pnpm *)",
                    "Bash(node *)",
                    "Bash(tsx *)",
                    "Bash(tsc *)",
                    "Bash(biome *)",
                ][..],
            ),
            (
                "jvm-dev",
                &[
                    "Bash(./gradlew *)",
                    "Bash(gradle *)",
                    "Bash(./mvnw *)",
                    "Bash(mvn *)",
                    "Bash(google-java-format *)",
                    "Bash(ktlint *)",
                ][..],
            ),
        ] {
            let p = PermissionProfile::from_str(name).expect("profile must exist");
            for rule in expected {
                assert!(p.allow.contains(rule), "{name} must allow {rule}");
            }
        }
        // Env-runners execute an arbitrary inner command, so they are scoped
        // per project rather than granted wholesale.
        assert!(!typescript_dev().allow.contains(&"Bash(npx *)"));
        // The JVM's build tool is the entry point for compile, test, and run,
        // so a bare `java` grant would reach `java -jar <anything>` for almost
        // no benefit.
        assert!(!jvm_dev().allow.contains(&"Bash(java *)"));
    }

    #[test]
    fn workspace_carries_the_language_agnostic_floor() {
        let p = workspace();
        for rule in [
            "Edit",
            "Write",
            "Bash(mkdir -p *)",
            "Bash(git commit *)",
            "Bash(harness *)",
        ] {
            assert!(p.allow.contains(&rule), "workspace must allow {rule}");
        }
        assert!(!p.allow.contains(&"Bash(cp *)"));
        assert!(!p.allow.contains(&"Bash(mv *)"));
    }

    #[test]
    fn a_dev_profile_carries_nothing_but_its_toolchain() {
        // The floor belongs to `workspace`, which is what a stack with no
        // language profile receives. A `<lang>-dev` profile that also carried
        // `Edit` or `git commit` would make those grants look like language
        // facts and leave the no-profile scaffold with an empty allow list.
        let floor: std::collections::HashSet<&str> = workspace().allow.into_iter().collect();
        for name in dev_profile_names() {
            let p = PermissionProfile::from_str(name).unwrap();
            for rule in &p.allow {
                assert!(
                    !floor.contains(rule),
                    "profile '{name}' repeats the workspace floor rule '{rule}'"
                );
            }
            // read-only built-ins are never granted (they never prompt)
            assert!(
                !p.allow.iter().any(|a| a.starts_with("Bash(ls")
                    || a.starts_with("Bash(grep")
                    || a.starts_with("Bash(git status")),
                "{name} must not grant no-op read-only built-ins"
            );
        }
    }

    #[test]
    fn composition_baseline_gcp_rust_merges_all_lists() {
        use crate::config::PermissionsPolicy;
        use crate::policy::permissions::PermissionGenerator;

        let policy = PermissionsPolicy {
            profiles: vec!["baseline".into(), "gcp-strict".into(), "rust-dev".into()],
            ..Default::default()
        };
        let block = PermissionGenerator::new(&policy).unwrap().generate();
        // baseline deny must be present
        assert!(block.deny.iter().any(|d| d == "Bash(sudo *)"));
        // gcp-strict deny must be present
        assert!(
            block
                .deny
                .iter()
                .any(|d| d == "Bash(gcloud projects delete *)")
        );
        // rust-dev allow must be present
        assert!(block.allow.iter().any(|a| a == "Bash(cargo *)"));
        // gcp-strict ask must be present
        assert!(block.ask.iter().any(|a| a == "Bash(gcloud * deploy *)"));
    }
}
