---
id: root-cause
applies_to: [code, spec, plan]
anchors:
  - constitution
---

# Root cause

Does the fix remove the cause, or hide the symptom?

- No band-aid masking a design flaw: a null guard over a should-never-be-null,
  a retry over a resource leak, a broad catch swallowing a real error.
- If the cause lives in another module, the fix goes there — not a workaround
  at the call site.
- Sibling code paths with the same latent cause are checked.
