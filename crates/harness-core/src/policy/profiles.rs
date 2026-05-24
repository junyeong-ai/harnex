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
        "git-strict",
        "gcp-strict",
        "aws-strict",
        "rust-dev",
        "python-dev",
        "typescript-dev",
    ];

    pub fn from_str(name: &str) -> Option<Self> {
        Some(match name {
            "baseline" => baseline(),
            "git-strict" => git_strict(),
            "gcp-strict" => gcp_strict(),
            "aws-strict" => aws_strict(),
            "rust-dev" => rust_dev(),
            "python-dev" => python_dev(),
            "typescript-dev" => typescript_dev(),
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
/// canonical space-then-`*` wildcard (`Bash(cmd *)`); Read/Edit/Write use
/// gitignore-style globs where a bare pattern matches at any depth
/// (`Read(.env)` ≡ `Read(**/.env)`). Two redundancy rules the spec lets us
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
            // --- sensitive file writes + edits (corruption guard). Write/Edit
            // denies are PRECISE deployment shapes — not broad `.env.*` —
            // because `deny > allow` makes broad denies unoverridable, so a
            // blanket `Write(.env.*)` would block legitimate scaffolding of
            // `.env.example` / `.env.sample` / `.env.template` with no
            // project-level escape hatch. The read denies above are broader
            // because exfiltration is the concern there, and reading
            // `.env.example` is unlikely to be programmatic.
            //
            // Deployment-env deny shapes cover the common naming conventions;
            // projects with additional deployment env names add them via
            // `[policy.permissions].extra_deny`. ---
            "Write(.env)",
            "Write(.env.local)",
            "Write(.env.development)",
            "Write(.env.staging)",
            "Write(.env.production)",
            "Write(*.pem)",
            "Write(*.key)",
            "Write(*.p12)",
            "Write(*.pfx)",
            "Write(/secrets/**)",
            "Write(~/.ssh/*)",
            "Write(~/.aws/credentials)",
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
            "Bash(git add .)",
            "Bash(git add -A *)",
            "Bash(git add -u *)",
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

/// File and shell operations every language-specific dev profile grants.
///
/// Only commands that actually prompt are listed: `Edit`/`Write` (file
/// modification requires approval) and the mutating filesystem/git commands.
/// Read-only built-ins (`ls`, `grep`, `find`, `cat`, `diff`, `wc`, read-only
/// `git status`/`diff`/`log`/`show`, …) are omitted — Claude Code runs them
/// without a prompt in every mode, so an allow rule would be a no-op. The
/// destructive git forms denied by `baseline` still win under deny > allow.
const COMMON_DEV_ALLOW: &[&str] = &[
    "Edit",
    "Write",
    "Bash(mkdir -p *)",
    "Bash(cp *)",
    "Bash(mv *)",
    "Bash(git add *)",
    "Bash(git commit *)",
    "Bash(git branch *)",
    "Bash(git stash *)",
    "Bash(git checkout -b *)",
    "Bash(git switch *)",
];

/// Rust development toolchain: cargo (covers `cargo clippy`/`cargo fmt`)
/// plus standalone rustfmt, on top of the common dev allows.
fn rust_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec!["Bash(cargo *)", "Bash(rustfmt *)"];
    allow.extend_from_slice(COMMON_DEV_ALLOW);
    PermissionProfile {
        name: "rust-dev",
        allow,
        ask: vec![],
        deny: vec![],
    }
}

/// Python development toolchain: uv, python, pytest, ruff, mypy, on top of
/// the common dev allows. `python -c` stays denied by `baseline`.
fn python_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec![
        "Bash(uv *)",
        "Bash(python *)",
        "Bash(pytest *)",
        "Bash(ruff *)",
        "Bash(mypy *)",
    ];
    allow.extend_from_slice(COMMON_DEV_ALLOW);
    PermissionProfile {
        name: "python-dev",
        allow,
        ask: vec![],
        deny: vec![],
    }
}

/// TypeScript development toolchain: pnpm, node, tsx, tsc, biome, on top of
/// the common dev allows. `node -e` stays denied by `baseline`. The broad
/// `npx *` is deliberately excluded — env-runners execute arbitrary inner
/// commands, so the spec advises a specific `Bash(npx <tool> *)` rule, which
/// the skill adds per project rather than granting wholesale here.
fn typescript_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec![
        "Bash(pnpm *)",
        "Bash(node *)",
        "Bash(tsx *)",
        "Bash(tsc *)",
        "Bash(biome *)",
    ];
    allow.extend_from_slice(COMMON_DEV_ALLOW);
    PermissionProfile {
        name: "typescript-dev",
        allow,
        ask: vec![],
        deny: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(p.deny.contains(&"Bash(git add .)"));
        assert!(p.deny.contains(&"Bash(git add -A *)"));
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
        // Write + Edit (corruption). Denies are PRECISE deployment shapes,
        // not broad `.env.*`, because `deny > allow` makes broad denies
        // unoverridable — scaffolding `.env.example` would be blocked with
        // no project-level escape hatch. `*credentials*.json` is
        // intentionally Read-only (mock credential fixtures for tests are a
        // legitimate Write target).
        for shape in [
            "Write(.env)",
            "Write(.env.local)",
            "Write(.env.development)",
            "Write(.env.staging)",
            "Write(.env.production)",
            "Write(*.pem)",
            "Write(*.key)",
            "Write(*.p12)",
            "Write(*.pfx)",
            "Write(/secrets/**)",
            "Write(~/.ssh/*)",
            "Write(~/.aws/credentials)",
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
            "Write(.env.example)",
            "Write(.env.sample)",
            "Write(.env.template)",
        ] {
            assert!(
                !p.deny.contains(&safe),
                "scaffolding shape '{safe}' must not be denied by baseline"
            );
        }
    }

    #[test]
    fn dev_profile_allows_never_contradict_baseline_deny() {
        // Safety invariant: no `*-dev` allow may be the exact string of a
        // baseline deny (deny > allow wins regardless, but an exact dup is a
        // config smell the auditor flags). Locks the property across edits.
        let deny: std::collections::HashSet<&str> = baseline().deny.into_iter().collect();
        for name in PermissionProfile::ALL {
            if name.ends_with("-dev") {
                let p = PermissionProfile::from_str(name).unwrap();
                for a in &p.allow {
                    assert!(
                        !deny.contains(a),
                        "profile '{name}' allow '{a}' is also a baseline deny"
                    );
                }
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
    fn rust_dev_allow_list_is_non_empty() {
        let p = rust_dev();
        assert!(!p.allow.is_empty(), "rust-dev must have allow patterns");
        assert!(p.allow.contains(&"Bash(cargo *)"));
        assert!(p.allow.contains(&"Bash(rustfmt *)"));
    }

    #[test]
    fn python_dev_allow_list_is_non_empty() {
        let p = python_dev();
        assert!(!p.allow.is_empty(), "python-dev must have allow patterns");
        assert!(p.allow.contains(&"Bash(uv *)"));
        assert!(p.allow.contains(&"Bash(python *)"));
        assert!(p.allow.contains(&"Bash(pytest *)"));
        assert!(p.allow.contains(&"Bash(ruff *)"));
        assert!(p.allow.contains(&"Bash(mypy *)"));
    }

    #[test]
    fn typescript_dev_allow_list_is_non_empty() {
        let p = typescript_dev();
        assert!(
            !p.allow.is_empty(),
            "typescript-dev must have allow patterns"
        );
        assert!(p.allow.contains(&"Bash(pnpm *)"));
        assert!(p.allow.contains(&"Bash(node *)"));
        assert!(p.allow.contains(&"Bash(tsx *)"));
        assert!(p.allow.contains(&"Bash(tsc *)"));
        assert!(p.allow.contains(&"Bash(biome *)"));
        // env-runners are scoped per project, never granted wholesale
        assert!(!p.allow.contains(&"Bash(npx *)"));
    }

    #[test]
    fn dev_profiles_include_common_dev_allows() {
        for name in &["rust-dev", "python-dev", "typescript-dev"] {
            let p = PermissionProfile::from_str(name).unwrap();
            assert!(p.allow.contains(&"Edit"), "{name} must allow Edit");
            assert!(p.allow.contains(&"Write"), "{name} must allow Write");
            assert!(
                p.allow.contains(&"Bash(git commit *)"),
                "{name} must include git commit"
            );
            assert!(
                p.allow.contains(&"Bash(mkdir -p *)"),
                "{name} must include mkdir"
            );
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
