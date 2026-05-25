---
paths:
  - "**/*.py"
  - "**/*.ts"
  - "**/*.tsx"
---

# Observability discipline

## Span naming

- Format: `<package>.<module>.<operation>` (dot-separated, lowercase).
- Avoid generic names (`process`, `handle`, `run`) — include the domain
  noun (`order.validate`, `embedding.generate`).

## Attribute conventions

- Use OpenTelemetry semantic conventions where they exist.
- Project-specific attributes use a namespace prefix
  (`myproject.user_id`, not bare `user_id`).
- Never emit credentials, tokens, or PII as attribute values.

## PII boundary

- Logs and traces operate inside the PII boundary — redact before emit.
- Structured logging only (`logger.info("msg", extra={...})`); never
  f-string interpolation of user data into log messages.

## Maturity model

1. **Observe first** — instrument before alerting. An alert without
   baseline data fires on noise.
2. **Baseline** — collect 2+ weeks of data before setting thresholds.
3. **Alert** — threshold-based alerts with documented runbooks.

<!-- Customize: replace package/attribute prefixes with your project's
     namespace. Add domain-specific span conventions as needed. -->
