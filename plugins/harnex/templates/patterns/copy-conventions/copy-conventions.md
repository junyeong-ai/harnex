---
paths:
  - "**/*.md"
  - "**/*.json"
  - "**/*.ts"
  - "**/*.tsx"
---

# Copy and communication conventions

Written communication standards across code, docs, and UI. Defaults below
are minimal and language-neutral; override with your project's locale and
tone.

## Register

- **Code comments / commit messages**: concise, imperative ("Add handler",
  not "Added handler" or "Adding handler").
- **User-facing text**: match the product's voice — decide once, document
  here, apply everywhere.
- **Documentation**: declarative present tense ("The handler validates
  input", not "The handler will validate input").

## Error messages

Format: **what went wrong → failing value → remediation hint**.

```
"connection refused: host={host} port={port} — check that the service
 is running and the firewall allows the port"
```

- Start with the symptom, not the action ("connection refused", not
  "failed to connect").
- Include the failing value when safe (no PII, no credentials).
- End with a remediation hint when possible.
- Never expose internal stack traces in user-facing errors.

## Number formatting

- Data contexts (tables, metrics, dashboards): digits (`3`, `1,024`).
- Prose: words for 1-9 ("three items"), digits for 10+ ("12 items").
- Units: space between number and unit (`100 ms`, `2.5 GB`).

## Terminology namespace

<!-- Fill in: one canonical term per concept. Example:
     - "사용자" / "user"   (not "유저", "이용자", "계정")
     - "대시보드" / "dashboard"  (not "관리화면", "admin panel")
     Aliases redirect to the canonical term. -->

## Localization

<!-- Fill in if multi-language: i18n framework, message key naming
     convention, pluralization rules. Delete this section if
     single-language. -->
