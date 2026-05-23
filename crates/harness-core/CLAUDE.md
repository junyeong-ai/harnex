# harness-core

Library crate. All logic lives here; `harness-cli` is a thin envelope
wrapper over these types.

## Recurring patterns

### Strategy enum (single source of truth)

Each closed set of named strategies follows the same shape:

```rust
pub enum FooStrategy { Bar, Baz }

impl FooStrategy {
    pub const ALL: &'static [Self] = &[Self::Bar, Self::Baz];
    pub fn from_str(s: &str) -> Option<Self> { /* exhaustive match */ }
    pub fn as_str(self) -> &'static str { /* exhaustive match */ }
}
```

Both `Config::validate_*` (string → enum) and the factory function
(enum → trait object) consume the enum. Adding a variant forces both
sites to update via the compiler's exhaustive-match check. There is no
parallel `KNOWN_*` const — that pattern was deleted because it drifts.

Instances of this pattern:
`VerifierStrategy`, `RendererStrategy`, `ConsumerStrategy`,
`PromotionDecision`, `FixCommand`, `SchemaTarget`, `PermissionProfile`,
`StorageKind`.

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
| `Verifier` | 4 verifier impls dispatched at runtime per claim shape |
| `Renderer` | 3 sentinel-block renderers per output format |
| `ConsumerDetector` | 2 strategies (grep / graph-backlinks), each anchored at construction |
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
