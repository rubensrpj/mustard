---
name: design-craft
description: "Unified design skill for ALL UI work: interfaces, dashboards, landing pages, marketing, components, posters. Domain exploration + intent-first methodology + craft principles + design system generation. Covers web, mobile, and desktop. Use when asked to build, design, or style any visual interface."
---

# Design Craft

Build every interface with craft, intention, and distinction. This skill covers ALL UI work — dashboards, landing pages, admin panels, marketing sites, components, mobile screens, and desktop apps.

---

# The Problem

You will generate generic output. Your training has seen thousands of dashboards and landing pages. The patterns are strong.

You can follow the entire process below — explore the domain, name a signature, state your intent — and still produce a template. This happens because intent lives in prose, but code generation pulls from patterns. The gap between them is where defaults win.

The process below helps. But process alone doesn't guarantee craft. You have to catch yourself.

---

# Where Defaults Hide

Defaults don't announce themselves. They disguise themselves as infrastructure.

**Typography feels like a container.** But typography isn't holding your design — it IS your design. A bakery management tool and a trading terminal might both need "clean, readable type" — but the type that's warm and handmade is not the type that's cold and precise.

**Navigation feels like scaffolding.** But navigation isn't around your product — it IS your product. Where you are, where you can go, what matters most.

**Data feels like presentation.** But a number on screen is not design. A progress ring and a stacked label both show "3 of 10" — one tells a story, one fills space.

**Token names feel like implementation detail.** But `--ink` and `--parchment` evoke a world. `--gray-700` and `--surface-2` evoke a template.

The moment you stop asking "why this?" is the moment defaults take over.

---

# Design Thinking

Before coding, commit to a BOLD direction. Answer these out loud:

**Who is this human?** Not "users." The actual person. Where are they? What's on their mind? A teacher at 7am with coffee is not a developer debugging at midnight.

**What must they accomplish?** Not "use the dashboard." The verb. Grade these submissions. Find the broken deployment. Approve the payment.

**What should this feel like?** Say it in words that mean something. "Clean and modern" means nothing. Warm like a notebook? Cold like a terminal? Dense like a trading floor? Calm like a reading app?

**What makes this UNFORGETTABLE?** What's the one thing someone will remember?

**CRITICAL**: Choose a clear conceptual direction and execute with precision. Bold maximalism and refined minimalism both work — the key is intentionality, not intensity.

If you cannot answer these with specifics, stop. Ask the user. Do not default.

---

# Product Domain Exploration

**Do not propose any direction until you produce all four:**

1. **Domain:** Concepts, metaphors, vocabulary from this product's world. Not features — territory. Minimum 5.
2. **Color world:** What colors exist naturally in this product's domain? Not "warm" or "cool" — go to the actual world. List 5+.
3. **Signature:** One element — visual, structural, or interaction — that could only exist for THIS product.
4. **Defaults:** 3 obvious choices for this interface type — visual AND structural. You can't avoid patterns you haven't named.

Your direction must explicitly reference all four. **The test:** Remove the product name. Could someone identify what this is for? If not, it's generic.

---

# Design System Generation

When the user requests a design system or you're starting a new product's visual direction:

1. **Read** `references/styles-catalog.md` — match product category to UI style
2. **Read** `references/palettes-catalog.md` — select or adapt palette for the domain
3. **Read** `references/typography-catalog.md` — pick font pairing matching the direction
4. **Read** `references/ux-guidelines.md` — apply CRITICAL guidelines from the start

Generate a complete token system: colors, typography, spacing, borders, shadows, border-radius. Every token should be explainable and traceable to a design decision.

---

# Frontend Aesthetics

Focus on these to avoid generic AI output:

- **Typography**: Choose fonts that are beautiful, unique, and interesting. Avoid generic fonts like Arial, Inter, Roboto. Pair a distinctive display font with a refined body font. See `references/typography-catalog.md`.
- **Color & Theme**: Commit to a cohesive aesthetic. Use CSS variables. Dominant colors with sharp accents outperform timid, evenly-distributed palettes.
- **Motion**: Use animations for high-impact moments. One well-orchestrated page load with staggered reveals creates more delight than scattered micro-interactions. Scroll-triggering and hover states that surprise.
- **Spatial Composition**: Unexpected layouts. Asymmetry. Overlap. Diagonal flow. Grid-breaking elements. Generous negative space OR controlled density.
- **Backgrounds & Visual Details**: Create atmosphere — gradient meshes, noise textures, geometric patterns, layered transparencies, dramatic shadows, decorative borders, grain overlays.

**NEVER:** Overused font families (Inter, Roboto, Arial, system fonts), cliched color schemes (purple gradients on white), predictable layouts, cookie-cutter design. No two designs should look the same.

---

# Craft Foundations

## Subtle Layering

The backbone of craft. You should barely notice the system working. When you look at Vercel's dashboard, you don't think "nice borders." You just understand the structure.

- **Surfaces stack** — base → cards → dropdowns → overlays. Each jump: few percentage points of lightness only.
- **Sidebars** — same background as canvas, not different. Subtle border is enough.
- **Inputs** — slightly darker than surroundings ("inset" feel).
- **Borders** — low opacity rgba that disappears when you're not looking for it.

## Infinite Expression

Every pattern has infinite expressions. A metric display could be a hero number, inline stat, sparkline, gauge, progress bar, comparison delta, trend badge. Linear's cards don't look like Notion's. Vercel's metrics don't look like Stripe's.

**If you swapped your choices for the most common alternatives and the design didn't feel different, you never made real choices.**

## Color Lives Somewhere

Every product exists in a world with colors. Before reaching for a palette, spend time in the product's world. Your palette should feel like it came FROM somewhere — not applied TO something.

---

# Every Choice Must Be A Choice

For every decision: WHY this layout? WHY this color temperature? WHY this typeface? WHY this spacing? If your answer is "it's common" or "it's clean" — you haven't chosen. You've defaulted.

**Sameness Is Failure.** If another AI would produce substantially the same output — you have failed.

**Intent Must Be Systemic.** If the intent is warm: surfaces, text, borders, accents, typography — ALL warm. Check your output against your stated intent.

---

# Before Writing Each Component

**Every time** you write UI code — state:

```
Intent: [who, what must they do, how should it feel]
Palette: [colors — and WHY they fit this product's world]
Depth: [borders / shadows / layered — and WHY]
Surfaces: [elevation scale — and WHY this temperature]
Typography: [typeface — and WHY it fits the intent]
Spacing: [base unit]
```

This checkpoint is mandatory. If you can't explain WHY for each choice, you're defaulting.

---

# The Mandate

**Before showing the user, look at what you made.**

Ask yourself: "If they said this lacks craft, what would they mean?" Fix it first.

Run these checks:
- **Swap test:** Swap the typeface for your usual one. Would anyone notice? The places where swapping wouldn't matter = where you defaulted.
- **Squint test:** Blur your eyes. Can you still perceive hierarchy? Is anything harsh?
- **Signature test:** Point to five specific elements where your signature appears.
- **Token test:** Read CSS variables out loud. Do they sound like they belong to this product's world?

If any check fails, iterate before showing.

---

# Workflow

Be invisible. Don't announce modes or narrate process.

1. **Explore domain** — Produce all four required outputs (domain, color world, signature, defaults)
2. **Propose** — Direction must reference all four. Lead with recommendation, then confirm.
3. **Build** — Apply principles from `references/principles.md`
4. **Evaluate** — Run mandate checks before showing
5. **Offer to save** — "Want me to save these patterns for future sessions?"

If project has `.interface-design/system.md` — read and apply. Decisions are made.

---

# Design Principles

See `references/principles.md` for detailed craft principles covering:
- Surface & Token Architecture
- Spacing System, Padding, Border Radius
- Depth & Elevation Strategy
- Card Layouts, Controls, Typography
- Animation, Iconography, Navigation Context
- Dark Mode

---

# Deep Dives

- `references/principles.md` — Core craft principles with code examples
- `references/critique.md` — Post-build design critique protocol
- `references/example.md` — Real-world craft thinking in action
- `references/validation.md` — Memory management, when to update system.md
- `references/styles-catalog.md` — 12 UI styles with reasoning and tokens
- `references/palettes-catalog.md` — Color palettes by product category
- `references/typography-catalog.md` — 24 curated font pairings
- `references/ux-guidelines.md` — UX guidelines by priority (accessibility, touch, performance)
