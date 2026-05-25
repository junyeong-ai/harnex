---
paths:
  - ".github/**"
---

# Pull request conventions

Every PR carries structured metadata so reviewers (human or AI) can
assess scope, impact, and risk without reading every diff line.

## Required sections

| Section | Purpose |
|---|---|
| **TL;DR** | One sentence: what changed and why. |
| **What changed** | Bullet list of concrete changes (file/module level). |
| **Impact** | What breaks if this is wrong? Blast radius. |
| **Risk** | Low / Medium / High + one-line justification. |

## AI-fill discipline

When Claude authors a PR description, it fills the sections from the
diff — never invents impact or risk. If uncertain, it states the
uncertainty explicitly ("Risk: Medium — untested path in production
auth flow").

## Review depth routing

| Risk | Review depth |
|---|---|
| Low | Automated checks sufficient |
| Medium | One human reviewer |
| High | Two reviewers + explicit test plan |

<!-- Customize: add project-specific sections (e.g., "Deploy steps",
     "Database migration", "Feature flag") as your team needs them. -->
