---
# <!-- harnex-fill: schedule the first tick — when: YYYY-MM-DD and
#      produces: the record path, e.g. .harness/records/<year>-q<n>-signal-census.md -->
cadence: quarterly
owner: harness
prompt: |
  Census the signals this harness emits against the readers that consume
  them. Run harnex telemetry report per declared kind and name every kind
  that was written in the window and read by nothing; for each, either
  name its reader in the record or open a retirement observation — a
  write-only signal is cost wearing the clothes of insight.
---

# Record
