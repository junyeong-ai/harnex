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

This is the only command that may emit non-envelope output, and only
behind `--raw`. Other commands MUST always emit envelopes (constitution
Article II).

Supported shells delegate to `clap_complete::Shell::value_variants()` —
bash, zsh, fish, powershell, elvish.
