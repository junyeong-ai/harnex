---
paths:
  - "crates/harness-cli/src/commands/completions.rs"
---

# completions — shell completion emission

`harness completions <shell>` returns the completion script. Default mode
wraps the script in the standard JSON envelope (`data.shell` + `data.script`)
so AI consumers stay on the predictable contract. `--raw` flag emits the
script directly for shell consumption:

```bash
harness completions bash --raw > ~/.bashrc.d/harness
harness completions zsh  --raw > ~/.zsh/completions/_harness
```

`--raw` is one of the sanctioned non-envelope exceptions (the full list and
the Article II contract live in `.claude/rules/envelope.md` — do not restate
it here). Without `--raw`, this command emits an envelope like every other.

Supported shells delegate to `clap_complete::Shell::value_variants()` — never
hand-maintain the shell list here.
