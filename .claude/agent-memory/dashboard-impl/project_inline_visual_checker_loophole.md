---
name: inline-visual-checker-loophole
description: check-pages-no-inline-visual.mjs only walks JSX className attribute values directly; class strings stored in `const foo = "..."` variables and then composed via template literals are NOT tokenized, so tokens like `bg-primary/10` pass when used that way but fail when written inline.
metadata:
  type: project
---

`scripts/check-pages-no-inline-visual.mjs` (AC-10 of `2026-05-23-dashboard-design-system`) only validates string/template literals that appear DIRECTLY as a JSX `className=` value. Class tokens stashed in a local `const active = "bg-primary/10 text-primary font-medium";` and then injected via a template literal `className={\`\${pillBase} \${active}\`}` are invisible to the checker — the `Identifier` reference is not a `Literal`/`TemplateLiteral` quasi.

**Why:** The collector in `collectClassNameTokens` recurses through CallExpression / ArrayExpression / Conditional / Logical, but it does NOT resolve Identifier references back to their declarations. So referenced strings are skipped.

**How to apply:**
- Direct inline tokens like `className="bg-primary/10 text-primary"` will FAIL the checker because `bg-primary/10` is not in TOKEN_WHITELIST (only `bg-primary` without slash) and not matched by any STRUCTURAL regex.
- The same tokens stored in a `const x = "..."` variable and composed via template literals PASS — see `Specs.tsx` (Wave 5) for examples.
- Allowed prefixes for state recursion: `(focus|focus-visible|hover|active|disabled):` ONLY. `last:`, `group:`, `peer:`, `dark:`, etc. will FAIL.
- Negative arbitrary tokens like `-translate-y-1/2` FAIL (the structural regex requires leading word char). Use `inset-y-0 my-auto` to center instead.
- `text-muted-foreground/N` and `text-card-foreground/N` are explicitly allowed via the dedicated regex. Other semantic tokens (`text-primary`, `bg-card`) do NOT support slash modifiers when used inline.
