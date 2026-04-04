# Plan: Add Boundary Protection per Spec (P3)

## Objective
1. Add `## Boundaries` spec section docs to `feature/SKILL.md` and `bugfix/SKILL.md`
2. Add `checkBoundaries()` to `guard-verify.js` for advisory (non-blocking) enforcement

---

## File 1: `templates/commands/mustard/feature/SKILL.md`

**Where to insert:** After the PLAN phase header content, specifically after the Full Scope spec format description (around line 84, after the checkpoint fields block) and after the Light Scope spec format block (around line 104). Best placement: in the PLAN phase, after the Full Scope `spec.md` contents description and Light Scope spec format — add a shared "Spec Boundaries" sub-section as part of the spec template documentation.

**Precise insertion point:** After line 84 (`4. Elegance Check...`) in Full Scope, the spec template already lists fields. The spec Boundaries doc should live inside the PLAN Phase section. Looking at the structure, the cleanest insertion is **after the full scope step 1 spec content** (after the checkpoint fields line, step 2) as a new note item, and similarly call it out in light scope.

Actually, re-reading the spec: the task says to add the content to the PLAN phase where the spec template is defined. Both Full Scope (step 1) and Light Scope (step 1 compact format) define spec content. The addition should be a standalone sub-section `#### Spec Boundaries` placed after the Full Scope block (after step 5 "Present to user...") and a brief reference in Light Scope.

**Exact location:** After `#### Light Scope` block ends (line 108, just before `### EXECUTE Phase`), insert `#### Spec Boundaries` section.

This is the cleanest spot — it applies to both Full and Light scope specs and is read before EXECUTE.

---

## File 2: `templates/commands/mustard/bugfix/SKILL.md`

**Where to insert:** The bugfix pipeline has a simpler structure — no formal Full/Light spec section like feature. The Full Path mentions writing a brief spec. Best placement: after `**Full Path:**` line (line 31) or as a new sub-section after the ANALYZE section. Given the spec says "Include a `## Boundaries` section in the bugfix spec", insert a `#### Spec Boundaries` block after the ANALYZE section and before EXECUTE — specifically after line 32 (the `**Full Path:**` description ends) and before `### EXECUTE`.

---

## File 3: `templates/hooks/guard-verify.js`

**Changes:**
1. Add `const fs = require('fs');` — currently only `path` and `_lib/hook-env` are required. `fs` is needed for `existsSync`, `readdirSync`, `readFileSync`.
2. Add `checkBoundaries(filePath, cwd)` function after `analyzeImports()`.
3. In the main handler, after the `violations.length > 0` block (but in the `else` branch), call `checkBoundaries` and return advisory warning if hit.

**Exact location for boundary call:** Replace the final `else` branch:
```js
} else {
  process.stdout.write(JSON.stringify({ decision: "approve" }));
}
```
With:
```js
} else {
  var boundaryViolation = checkBoundaries(filePath, ROOT);
  if (boundaryViolation) {
    process.stdout.write(JSON.stringify({
      decision: 'approve',
      reason: '[BOUNDARY WARNING] File "' + relPath + '" is listed in spec boundaries (DO NOT MODIFY: ' + boundaryViolation.boundary + ' — spec: ' + boundaryViolation.spec + '). Verify this edit is intentional.'
    }));
  } else {
    process.stdout.write(JSON.stringify({ decision: "approve" }));
  }
}
```

**`checkBoundaries` function:** Placed before the closing of the file, after `analyzeImports`. Uses `fs` which needs to be required at the top. Implementation follows the spec exactly with directory, glob, and exact-path matching.

**Note on `fs` require:** Currently the file only requires `path` and `_lib/hook-env`. Need to add `const fs = require('fs');` at the top.

---

## Guards Checklist
- [x] CommonJS only (no ESM)
- [x] No external deps (only `fs`, `path` — built-ins)
- [x] Fail-open: boundary check wrapped in try/catch, returns null on error
- [x] Advisory only: returns `{ decision: 'approve', reason: ... }` — never blocks
- [x] PostToolUse format: uses `decision` not `permissionDecision`
- [x] Windows paths normalized: `relPath` already uses `.replace(/\\/g, '/')`
- [x] Fast: reads spec once per active spec directory, simple string matching
- [x] Silent skip when no active spec or no Boundaries section
