# harness-core

Library crate. All logic lives here; `harness-cli` is a thin envelope
wrapper over these types.

## Recurring patterns

### Closed-set discriminator enum (single source of truth)

Each closed vocabulary follows the same shape:

```rust
pub enum FooKind { Bar, Baz }

impl FooKind {
    pub const ALL: &'static [Self] = &[Self::Bar, Self::Baz];
    pub fn from_str(s: &str) -> Option<Self> { /* exhaustive match */ }
    pub fn as_str(self) -> &'static str { /* exhaustive match */ }
}
```

Both `Config::validate_*` (string → enum) and the factory function
(enum → trait object) consume the enum. Adding a variant forces every
consuming site to update via the compiler's exhaustive-match check. No
parallel `KNOWN_*` const — that pattern is forbidden because it drifts.

**Suffix is chosen for what the enum names**, not for the pattern:
`Strategy` for swappable algorithms (`VerifierStrategy`, `RendererStrategy`,
`ConsumerStrategy`); `Format` for serialization (`SourceFormat`); `Kind`
for storage / shape (`StorageKind`, `PermissionFindingKind`); `Profile`
for composable policy (`PermissionProfile`); `Decision` for ledger
verdicts (`PromotionDecision`); `Command` for executable identity
(`FixCommand`); `Target` for output destination (`SchemaTarget`);
`Outcome` for terminal state (`HookRunOutcome`, `SyncOutcome`);
`Severity` for finding rank. The pattern is the SSoT + exhaustive
match, not the suffix.

### Finding-producer naming convention

Every class that ingests input and emits `Vec<Finding>` follows this
domain-suffix convention:

| Suffix | Means | Examples |
|---|---|---|
| `Validator` | frontmatter / structural shape check on a single file | `RuleValidator`, `SkillValidator`, `SettingsValidator`, `CommitMsgValidator` |
| `Auditor` | cross-input semantic / policy compliance check | `PermissionAuditor`, `ProjectAuditor`, `StopAuditor` |
| `Verifier` | provenance / claim verification | `EvidenceVerifier` |
| `Classifier` | enrichment that assigns a category | `RetirementClassifier` |
| `Sweeper` | top-level driver that fans out classifiers over kinds | `RetirementSweeper` |
| `Syncer` | drift detection between SSoT and projection | `SentinelSyncer` |
| `Recorder` | append-only ledger writer | `LifecycleDecisionRecorder` |
| `Generator` | composes config-declared inputs into one artifact | `PermissionGenerator` |

When introducing a new finding-producer, pick the suffix from this
table — never invent a new one without extending this table first.

### Adding an `ErrorCode` variant

1. Add the variant to `error::ErrorCode` and its `as_str()` arm.
2. Add the variant to the `all` array AND the exhaustive match in
   `export::error_code_strings()` — the match fails to compile without
   it. The array must also include the new variant or
   `error_codes_schema_lists_all_variants` test catches the gap.
3. Add a typed variant to `error::Error` with `#[error(...)]` mapping.

### Adding a finding emit site

Every `Finding` must carry an actionable `hint`. There are no
`hint: None` production sites — the audit guard is that the absence is
visible at code review time. If a finding is `auto_fixable`, its
`fix_command` MUST be a `FixCommand::*.as_str()` value (not a free
string), because `ProjectChecker::try_fix` dispatches via the enum.

### Trait abstractions in use

| Trait | Reason it exists (not speculation) |
|---|---|
| `Verifier` | runtime dispatch per claim shape |
| `Renderer` | one impl per sentinel-block output format |
| `ConsumerDetector` | grep + graph-backlinks, each anchored at construction |
| `NodexRunner` | external-process boundary + test mock seam (see `graph::client`) |

No 1-impl trait exists outside of a documented process/test boundary.
`Storage` was removed once it became 1-impl with no mock.

## Testing

- Unit tests colocated in `#[cfg(test)] mod tests` per file.
- Integration tests in `tests/` per module domain
  (`tests/lifecycle.rs`, `tests/evidence.rs`, …).
- Strategy enums get a `mod strategy_tests` with round-trip + reject-unknown.
- Naming: `<subject>_<verb>_<expected>` (e.g.,
  `promoter_lists_threshold_crossing_groups`,
  `demote_refused_without_prior_approval`).

## Module rules

Every `mod.rs` ships a `//!` doc with WHAT + WHAT-REFUSED sections.
See `.claude/rules/module-doc.md` for the contract; lib.rs is exempt
because it only declares modules.
