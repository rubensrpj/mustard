# Frontend Craft Check (anti-AI-look)

> Detail for `/feature` and `/bugfix` when role=ui. Loaded on demand.
> **Purpose:** distinguish "MVP gerado" from "produto polido". Read once before first Edit/Write of UI work.

---

## Why this exists

LLM-generated UI tends to leak a recognizable aesthetic: literal colors instead of design system tokens, missing empty/error states, no microinteractions, generic Lorem Ipsum, lazy a11y. This checklist surfaces those tells before review.

---

## Checklist (all UI tasks)

### 1. Design system fidelity
- [ ] Colors come from DS tokens (`var(--color-*)`, `theme.colors.*`, `tokens.color.*`) — NOT literal hex/rgb
- [ ] Spacing comes from DS scale (`var(--space-*)`, `theme.spacing(...)`) — NOT raw px (except 1px borders)
- [ ] Typography uses DS type scale (`text-{size}`, `theme.typography.*`) — NOT inline font-size/line-height
- [ ] Component reuses existing primitives (Button, Input, Card) before introducing new ones — grep `{ui}/components/` first
- [ ] Icons come from project's icon set — never inline SVG when set has equivalent

### 2. State coverage (the AI-look killer)
- [ ] **Loading** state explicit (skeleton or spinner — never blank)
- [ ] **Empty** state explicit (helpful message + optional CTA — never silent void)
- [ ] **Error** state recoverable (show retry/back; never just `console.error`)
- [ ] **Success** state has subtle confirmation (toast, inline check, transition — not a hard cut)
- [ ] **Disabled** state has aria-disabled + visible style (not just `pointer-events: none`)

### 3. Microinteractions
- [ ] Hover/focus/active styles for all interactive elements — distinct, not all the same
- [ ] Transitions on hover/focus (≤200ms; ease-out for entry, ease-in for exit)
- [ ] Click feedback (ripple, scale, color shift — match DS conventions)
- [ ] Form submit shows pending state (button disabled + spinner) — not silent
- [ ] `prefers-reduced-motion` respected (`@media (prefers-reduced-motion: reduce)` zeroes animations)

### 4. Accessibility (non-negotiable)
- [ ] Semantic HTML (button vs div+onClick; nav/main/aside; h1→h2→h3 hierarchy)
- [ ] All form inputs have `<label>` (visible or `aria-label`/`aria-labelledby`)
- [ ] Errors connected via `aria-describedby` + `role="alert"` for live regions
- [ ] Keyboard navigable: Tab order matches visual order; Esc closes modals; Enter submits forms
- [ ] Focus ring visible (don't `outline: none` without replacement)
- [ ] Color contrast ≥ AA (4.5:1 for body, 3:1 for large text) — verify via DS token, not by eye
- [ ] Images have `alt` (descriptive or `alt=""` for decorative — never missing)

### 5. Content quality
- [ ] **Zero Lorem Ipsum** in production-facing UI (placeholders OK in storybook only)
- [ ] **Zero generic placeholder names** (`John Doe`, `example@example.com` — replace with realistic domain content)
- [ ] Empty states have specific copy ("No invoices yet — create your first" not "No data")
- [ ] Error messages are actionable ("Email already in use" not "Validation failed")
- [ ] Microcopy reviewed: action labels are verbs; consistent voice/tense

### 6. Responsive + density
- [ ] Layout works at xs/sm/md/lg/xl breakpoints — test at min and max of each
- [ ] No horizontal scroll on any breakpoint (unless intentional, e.g., tables)
- [ ] Touch targets ≥ 44×44px on mobile (WCAG 2.5.5)
- [ ] Hover-only states have keyboard/touch equivalents

### 7. Performance hygiene
- [ ] No layout shift on async data load (reserve space with skeleton)
- [ ] Images use proper format (webp/avif when supported) and dimensions (avoid 4K thumbnails)
- [ ] Long lists virtualize (react-window/equivalent) when >100 items
- [ ] `key` on every list item is stable (not array index for ordered/filterable lists)

---

## Quick rejection signals (review-time)

If the review agent sees ANY of these in the diff, FLAG as CONCERN:

- Hex/rgb literal in JSX/CSS (use DS token)
- Empty state with `<div></div>` or no content
- `setTimeout` for animation (use CSS transition)
- `onClick` on `<div>` without role/keyboard handlers
- `lorem ipsum` / `John Doe` / `example.com` in non-storybook
- `outline: 0` / `outline: none` without `:focus-visible` replacement
- Missing `<label>` for `<input>`
- Inline `style={{ color: '#xxx' }}` for theme colors

---

## When NOT to apply this

- Internal admin tools where pragmatism > polish (still apply 1-4, skip 5-7)
- Storybook entries / sandbox pages (Lorem Ipsum and dev-only colors OK)
- Tests / debug pages
