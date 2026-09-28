# harness-cli

Thin clap binary over `harness-core`. Every command produces exactly one
JSON envelope on stdout and exits with a documented code, except the
`guard` commands that speak a hook contract instead — `.claude/rules/envelope.md`
enumerates them, and a copy of that list here is the one that goes stale.

## The envelope contract

- Success: `write_envelope_success(out, data)` — wraps in
  `{"ok":true,"data":<data>,"warnings":[]}`. Use
  `write_envelope_success_warned(out, data, warnings)` when a command
  succeeded and the operator is owed something anyway — a result that is
  correct and cannot yet be used.
- Error: harness-core `Error` flows up; main converts via the typed
  ErrorCode to `{"ok":false,"error":{...}}`. Invalid CLI arguments are
  caught via `Cli::try_parse()` and mapped to an error envelope (exit 2);
  `--help` / `--version` stay clap-native (exit 0).
- The sanctioned non-envelope stdout exceptions are enumerated in
  `.claude/rules/envelope.md`, each with the contract it speaks instead — do
  not add others, and do not restate that list.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success (no findings, or only advisory `Minor`/`Info` findings) |
| `1` | At least one gating `Finding` (`Severity::fails_gate()` — `Blocker` or `Major`), or any failure a result without a severity reports |
| `2` | Runtime failure (config not found, IO failure, invalid arguments) |

The gate threshold is the single source of truth `Severity::fails_gate()`
(returns true for `Blocker | Major`). Every command that reports `Finding`s
decides exit 1 via
`findings.iter().any(|f| f.severity.fails_gate())` — keep it identical across
sites. To change the threshold, edit `fails_gate`, never the call sites. A
result that carries no severity — a `PermissionFinding`, a codegen drift, a
version verdict — exits 1 on any failure it reports.

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
- No prose on stdout, and on stderr only where `.claude/rules/envelope.md`
  names the writer. The envelope is the only output (per `constitution.md`
  Article II).
- No direct `std::fs::write` — route through
  `harness_core::path_guard` (`write_atomic` or `append_line`) if a CLI
  handler must mutate state (rare; most state mutation lives in core).
