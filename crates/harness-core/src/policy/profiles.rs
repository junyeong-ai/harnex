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
fn baseline() -> PermissionProfile {
    PermissionProfile {
        name: "baseline",
        allow: vec![],
        ask: vec![],
        deny: vec![
            // --- sensitive file access (read + write) ---
            "Read(.env)",
            "Read(.env.*)",
            "Read(**/.env)",
            "Read(**/.env.*)",
            "Read(**/*credentials*.json)",
            "Read(**/*credentials*)",
            "Read(**/*secret*)",
            "Read(**/*.key)",
            "Read(**/*.pem)",
            "Read(**/.aws/credentials)",
            "Read(~/.ssh/*)",
            "Write(.env)",
            "Write(.env.*)",
            "Write(**/.env)",
            "Write(**/.env.*)",
            "Write(**/*.key)",
            "Write(**/*.pem)",
            "Write(**/*credentials*.json)",
            "Bash(cat .env:*)",
            "Bash(cat .env.*:*)",
            // --- destructive git ---
            "Bash(git push --force:*)",
            "Bash(git push --force *)",
            "Bash(git push -f:*)",
            "Bash(git push -f *)",
            "Bash(git reset --hard:*)",
            "Bash(git reset --hard *)",
            "Bash(git checkout .:*)",
            "Bash(git checkout -- .:*)",
            "Bash(git restore .:*)",
            "Bash(git restore -- .:*)",
            "Bash(git branch -D main:*)",
            "Bash(git branch -D master:*)",
            "Bash(git rebase -i:*)",
            "Bash(git rebase -i *)",
            "Bash(git clean -fd:*)",
            "Bash(git clean -fd *)",
            "Bash(git add .:*)",
            "Bash(git add -A:*)",
            "Bash(git add -A *)",
            "Bash(git add -u:*)",
            "Bash(git add -u *)",
            // --- arbitrary code execution ---
            "Bash(node -e *)",
            "Bash(node --eval *)",
            "Bash(python -c *)",
            "Bash(python3 -c *)",
            "Bash(find * -exec *)",
            "Bash(find * -delete)",
            "Bash(find * -ok *)",
            // --- filesystem destruction ---
            "Bash(rm -rf /)",
            "Bash(rm -rf /*)",
            "Bash(rm -rf ~)",
            "Bash(rm -rf ~/*)",
            "Bash(rm -rf $HOME)",
            "Bash(rm -rf $HOME/*)",
            "Bash(chmod -R 777 *)",
            "Bash(sudo *)",
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
            "Bash(terraform destroy:*)",
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

/// Common shell commands allowed for all language-specific dev profiles.
const COMMON_SHELL_ALLOW: &[&str] = &[
    "Bash(ls:*)",
    "Bash(find:*)",
    "Bash(grep:*)",
    "Bash(mkdir:*)",
    "Bash(cp:*)",
    "Bash(echo:*)",
    "Bash(diff:*)",
    "Bash(wc:*)",
    "Bash(sort:*)",
    "Bash(uniq:*)",
    "Bash(which:*)",
    "Bash(git status:*)",
    "Bash(git diff:*)",
    "Bash(git log:*)",
    "Bash(git add:*)",
    "Bash(git commit:*)",
    "Bash(git branch:*)",
    "Bash(git stash:*)",
    "Bash(git show:*)",
];

/// Rust development toolchain: cargo, rustfmt, clippy, plus common
/// shell commands.
fn rust_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec!["Bash(cargo:*)", "Bash(rustfmt:*)", "Bash(clippy:*)"];
    allow.extend_from_slice(COMMON_SHELL_ALLOW);
    PermissionProfile {
        name: "rust-dev",
        allow,
        ask: vec![],
        deny: vec![],
    }
}

/// Python development toolchain: uv, python, pytest, ruff, mypy, plus
/// common shell commands.
fn python_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec![
        "Bash(uv:*)",
        "Bash(python:*)",
        "Bash(pytest:*)",
        "Bash(ruff:*)",
        "Bash(mypy:*)",
    ];
    allow.extend_from_slice(COMMON_SHELL_ALLOW);
    PermissionProfile {
        name: "python-dev",
        allow,
        ask: vec![],
        deny: vec![],
    }
}

/// TypeScript development toolchain: pnpm, npx, node, tsx, tsc, biome,
/// plus common shell commands.
fn typescript_dev() -> PermissionProfile {
    let mut allow: Vec<&'static str> = vec![
        "Bash(pnpm:*)",
        "Bash(npx:*)",
        "Bash(node:*)",
        "Bash(tsx:*)",
        "Bash(tsc:*)",
        "Bash(biome:*)",
    ];
    allow.extend_from_slice(COMMON_SHELL_ALLOW);
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
        assert!(p.deny.contains(&"Bash(git push --force:*)"));
        assert!(p.deny.contains(&"Bash(git push -f:*)"));
        assert!(p.deny.contains(&"Bash(git reset --hard:*)"));
        assert!(p.deny.contains(&"Bash(git checkout .:*)"));
        assert!(p.deny.contains(&"Bash(git add .:*)"));
        assert!(p.deny.contains(&"Bash(git add -A:*)"));
        assert!(p.deny.contains(&"Bash(git clean -fd:*)"));
        assert!(p.deny.contains(&"Bash(git rebase -i:*)"));
    }

    #[test]
    fn baseline_includes_sensitive_file_denies() {
        let p = baseline();
        assert!(p.deny.contains(&"Read(.env)"));
        assert!(p.deny.contains(&"Read(.env.*)"));
        assert!(p.deny.contains(&"Read(**/*.key)"));
        assert!(p.deny.contains(&"Read(**/*.pem)"));
        assert!(p.deny.contains(&"Read(**/*credentials*.json)"));
        assert!(p.deny.contains(&"Write(**/*.key)"));
        assert!(p.deny.contains(&"Write(**/*.pem)"));
        assert!(p.deny.contains(&"Write(**/*credentials*.json)"));
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
        assert!(p.allow.contains(&"Bash(cargo:*)"));
        assert!(p.allow.contains(&"Bash(rustfmt:*)"));
        assert!(p.allow.contains(&"Bash(clippy:*)"));
    }

    #[test]
    fn python_dev_allow_list_is_non_empty() {
        let p = python_dev();
        assert!(!p.allow.is_empty(), "python-dev must have allow patterns");
        assert!(p.allow.contains(&"Bash(uv:*)"));
        assert!(p.allow.contains(&"Bash(python:*)"));
        assert!(p.allow.contains(&"Bash(pytest:*)"));
        assert!(p.allow.contains(&"Bash(ruff:*)"));
        assert!(p.allow.contains(&"Bash(mypy:*)"));
    }

    #[test]
    fn typescript_dev_allow_list_is_non_empty() {
        let p = typescript_dev();
        assert!(
            !p.allow.is_empty(),
            "typescript-dev must have allow patterns"
        );
        assert!(p.allow.contains(&"Bash(pnpm:*)"));
        assert!(p.allow.contains(&"Bash(npx:*)"));
        assert!(p.allow.contains(&"Bash(node:*)"));
        assert!(p.allow.contains(&"Bash(tsx:*)"));
        assert!(p.allow.contains(&"Bash(tsc:*)"));
        assert!(p.allow.contains(&"Bash(biome:*)"));
    }

    #[test]
    fn dev_profiles_include_common_shell_allows() {
        for name in &["rust-dev", "python-dev", "typescript-dev"] {
            let p = PermissionProfile::from_str(name).unwrap();
            assert!(p.allow.contains(&"Bash(ls:*)"), "{name} must include ls");
            assert!(
                p.allow.contains(&"Bash(git status:*)"),
                "{name} must include git status"
            );
            assert!(
                p.allow.contains(&"Bash(git diff:*)"),
                "{name} must include git diff"
            );
            assert!(
                p.allow.contains(&"Bash(grep:*)"),
                "{name} must include grep"
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
        assert!(block.allow.iter().any(|a| a == "Bash(cargo:*)"));
        // gcp-strict ask must be present
        assert!(block.ask.iter().any(|a| a == "Bash(gcloud * deploy *)"));
    }
}
