# harness-cli

Thin clap binary over `harness-core`. Every command produces exactly one
JSON envelope on stdout and exits with a documented code.

## The envelope contract

- Success: `write_envelope_success(out, data)` — wraps in
  `{"ok":true,"data":<data>,"warnings":[]}`.
- Error: harness-core `Error` flows up; main converts via the typed
  ErrorCode to `{"ok":false,"error":{...}}`. Invalid CLI arguments are
  caught via `Cli::try_parse()` and mapped to an error envelope (exit 2);
  `--help` / `--version` stay clap-native (exit 0).
- The sanctioned non-envelope stdout exceptions (`--raw`, `guard hook-run`
  passthrough, `guard stop-audit` exit 2) are enumerated in
  `.claude/rules/envelope.md` — do not add others.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (no findings, or only advisory `Minor`/`Info` findings) |
| `1` | At least one gating finding (`Severity::fails_gate()` — `Blocker` or `Major`) |
| `2` | Runtime failure (config not found, IO failure, invalid arguments) |

The gate threshold is the single source of truth `Severity::fails_gate()`
(returns true for `Blocker | Major`). `check.rs`, `audit.rs`, `evidence.rs`,
`validate.rs` all decide exit 1 via
`findings.iter().any(|f| f.severity.fails_gate())` — keep it identical across
sites. To change the threshold, edit `fails_gate`, never the call sites.

## Shell completions

`completions <shell>` delegates to `clap_complete::Shell::value_variants()` —
never hand-maintain the shell list.

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
