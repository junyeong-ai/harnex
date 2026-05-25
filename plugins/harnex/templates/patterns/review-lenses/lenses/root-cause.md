---
id: root-cause
applies_to: [code, spec, plan]
anchors:
  - constitution
---

# Root cause

- The fix addresses the underlying cause, not just the symptom.
- No band-aid patterns: null guards hiding a design flaw, retry loops
  masking a resource leak, catch-all exceptions swallowing real errors.
- If the root cause is in a different module/package, the fix goes there
  (not a workaround in the caller).
- Regression risk is addressed — similar code paths that might have the
  same issue are checked.
