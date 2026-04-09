# /task audit — Domain Checklists

Orchestrator infers domain from scope keywords. Multiple domains can be combined.

| Domain | Keywords | Checklist |
|--------|----------|-----------|
| `copy` | copy, text, wording, tone, marketing | Tone consistency, grammar, placeholder text, marketing claims accuracy, CTA clarity |
| `design` | design, tokens, colors, typography, UI | Token usage, component reuse, visual hierarchy, spacing consistency, dark/light parity |
| `a11y` | accessibility, a11y, aria, contrast | ARIA labels, contrast ratios, keyboard navigation, screen reader support, focus management |
| `i18n` | i18n, translation, locale, language | Missing keys across locales, hardcoded strings, parameter consistency, pluralization |
| `consistency` | consistency, naming, structure, patterns | Naming conventions, file structure, pattern adherence across modules |
| `api-contract` | api, contract, endpoint, dto | DTO completeness, status codes, error response format, endpoint naming, versioning |

Default domain (if ambiguous): `consistency`.
