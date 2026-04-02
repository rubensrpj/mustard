# Plan: Diagnostic Failure Routing (P2)

## Goal
Add a "classify before retry" failure routing step to three template files. Text/template-only changes — no code.

---

## File 1: `templates/pipeline-config.md`

**Insert location:** After the `## Compact Guidance` section (line 41), before `## Parallel Rules` (line 43).

**New section to insert:**

```markdown
## Diagnostic Failure Routing

When a task fails or an agent reports BLOCKED/NEEDS_CONTEXT, classify the root cause BEFORE attempting a fix:

| Classification | Signal | Action |
|---------------|--------|--------|
| **Intent** | Requirements unclear, user asked for X but meant Y, we're building the wrong thing | **Re-plan**: stop EXECUTE, ask user to clarify, rewrite affected spec section |
| **Spec** | Plan was incomplete, missed edge case, wrong file targeted, missing dependency | **Fix spec**: update the task definition, then re-dispatch agent with corrected instructions |
| **Code** | Plan was correct but implementation has bug, test fails, syntax error | **Fix in-place**: retry the same agent with error context (standard retry) |

### Routing Flow

```
Task fails
    │
    ▼
Classify: intent / spec / code?
    │
    ├── intent → STOP. Ask user to clarify. Re-plan affected tasks.
    ├── spec   → Update spec task definition. Re-dispatch agent with corrected spec.
    └── code   → Standard retry with error context (max 2 retries).
```

### Classification Heuristic

Ask these questions in order:
1. "Does the user's original request match what we're building?" → No = **intent**
2. "Does the spec/task describe the right approach?" → No = **spec**
3. "Is the spec right but the code wrong?" → Yes = **code**

### Token Economy

- **Blind retry** (old): ~5K-15K tokens per attempt, often repeated 2-3x
- **Classified retry** (new): ~300 tokens for classification, then targeted fix
- Net savings: avoids 1-2 unnecessary retry cycles per failure
```

---

## File 2: `templates/commands/mustard/feature/SKILL.md`

**Insert location:** After step 11 ("Failed → max 2 retries, then STOP + report") in the EXECUTE Phase section (line 124), before `## Visual Output` (line 126).

**New subsection to insert:**

```markdown
#### Failure Routing

When an agent fails or returns BLOCKED/NEEDS_CONTEXT during EXECUTE:

1. **Classify** the failure (ask in order):
   - Is the user's intent different from what we're building? → **Intent issue** → pause, clarify with user, re-plan
   - Is the spec/task wrong or incomplete? → **Spec issue** → fix the task in spec, re-dispatch
   - Is the code just buggy? → **Code issue** → retry with error context (max 2 retries)

2. **Do NOT** blindly retry. Classification costs ~300 tokens. A blind retry costs 5K-15K.

3. After 2 code-level retries with no progress → escalate to user as BLOCKED.
```

---

## File 3: `templates/commands/mustard/bugfix/SKILL.md`

**Insert location:** After the `#### Retry Compact Advisory` block (line 50), before `### CLOSE` (line 53).

**New subsection to insert:**

```markdown
#### Failure Routing (Bugfix)

When a fix attempt fails:

1. **Classify**:
   - Is the bug actually in a different area than we thought? → **Intent** → re-analyze root cause
   - Did we target the wrong file/function? → **Spec** → update the fix plan
   - Is the fix approach right but implementation wrong? → **Code** → retry with error context

2. **Do NOT** blindly retry. Classification costs ~300 tokens. A blind retry costs 5K-15K.

3. Max 2 code-level retries, then escalate to user as BLOCKED.
```

---

## Constraints Checklist
- [ ] No existing retry logic removed — only augmented
- [ ] Classification heuristic kept to 3 questions
- [ ] Token savings callout present in all three files
- [ ] `pipeline-config.md` gets the canonical definition; SKILL.md files reference the same logic
- [ ] No code changes — text/template only
- [ ] `pipeline-config.md` does NOT have `<!-- mustard:generated -->` header (manual file) — no header to worry about
- [ ] Both SKILL.md files already end with `ULTRATHINK` — preserve that

---

## Execution Order
1. `pipeline-config.md` — canonical definition
2. `feature/SKILL.md` — EXECUTE phase augmentation
3. `bugfix/SKILL.md` — EXECUTE phase augmentation
