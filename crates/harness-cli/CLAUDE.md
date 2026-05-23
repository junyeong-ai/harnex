# harness-cli

Thin clap binary over `harness-core`. Every command produces exactly one
JSON envelope on stdout and exits with a documented code.

## The envelope contract

- Success: `write_envelope_success(out, data)` — wraps in
  `{"ok":true,"data":<data>,"warnings":[]}`.
- Error: harness-core `Error` flows up; main converts via the typed
  ErrorCode to `{"ok":false,"error":{...}}`.
- The single exception is `harness completions --raw` (shell-completion
  bytes go straight to stdout; documented in
  `.claude/rules/completions.md`).

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (no findings, or findings below blocker severity) |
| `1` | At least one `Severity::Blocker` finding |
| `2` | Runtime failure (config not found, IO failure, etc.) |

The `has_blocker = findings.iter().any(|f| f.severity == Severity::Blocker)`
pattern is used in `check.rs`, `evidence.rs`, `validate.rs` — keep it
identical across sites for predictable AI/CI semantics.

## Adding a subcommand

1. Create `commands/<group>.rs` exposing a clap `enum SomeCommand` plus
   `pub fn run<W: Write>(cmd, out) -> Result<ExitCode>`.
2. Each match arm calls into `harness-core`, then `write_envelope_success`.
3. Register the group in `main.rs` `Cli::Command` enum.
4. For options that mirror a closed enum, use the enum's `ALL/as_str`:

   ```rust
   #[arg(long, value_parser = decision_kind_values())]
   decision: Option<String>,
   ```

   Backed by a free fn returning `Vec<&'static str>` derived from the
   enum's `ALL`. Hardcoded string lists drift; this pattern doesn't.

## What this crate refuses to do

- No business logic. Pure clap dispatch + envelope wrapping.
- No `println!` / `eprintln!` of human prose in the success path. The
  envelope is the only output (per `constitution.md` Article II).
- No direct `std::fs::write` — route through
  `harness_core::path_guard` (`write_atomic` or `append_line`) if a CLI
  handler must mutate state (rare; most state mutation lives in core).
