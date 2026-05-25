---
description: <one line — what this skill does AND when to use it. Claude reads this to decide auto-invocation; put the key use case first. description + when_to_use stay under 1536 chars.>
disable-model-invocation: true
---

<!-- harnex skill scaffold — fill the procedure, then delete this comment.
     - Keep this file under 500 lines; move long reference to sibling files in
       the skill directory and link them so they load only when needed.
     - `disable-model-invocation: true` stops Claude auto-running a skill that
       is unfinished or has side effects (deploy / commit / release). For a
       knowledge skill Claude SHOULD apply automatically, delete that line and
       sharpen `description` with trigger phrases instead.
     - Pre-approve only the tools the procedure needs with
       `allowed-tools: Bash(cmd *) ...` (it GRANTS, it does not restrict).
     - Reference bundled files by `${CLAUDE_SKILL_DIR}/...` so paths resolve
       wherever the skill is installed. -->

# <skill name>

## Steps

1. <first step — imperative, verifiable>
2. <next step>
