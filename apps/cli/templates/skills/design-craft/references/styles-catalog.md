# UI Styles Catalog

Reference catalog of UI styles for design system decisions. Match style to product context.

---

## Minimalism
Clean, restrained, content-focused. Maximum whitespace, limited color palette.
- **When:** SaaS dashboards, productivity tools, documentation platforms
- **Tokens:** Low border-radius (2-4px), generous padding, monochrome palette + 1 accent
- **Anti-patterns:** Empty feeling, lack of hierarchy, boring without craft

## Editorial / Magazine
Typography-driven, asymmetric layouts, strong visual hierarchy through type scale.
- **When:** Content platforms, blogs, landing pages, portfolios
- **Tokens:** Large display fonts, tight letter-spacing headlines, generous line-height body
- **Anti-patterns:** Over-styled headers with weak body, inconsistent type scale

## Glassmorphism
Frosted glass effect, background blur, semi-transparent surfaces.
- **When:** Modern dashboards, overlay-heavy UIs, card-based layouts
- **Tokens:** `backdrop-filter: blur(12-20px)`, rgba backgrounds (0.6-0.8 alpha), subtle borders
- **Anti-patterns:** Illegible text over busy backgrounds, overuse on every surface

## Claymorphism
Soft 3D, pastel colors, inner shadows creating clay-like depth.
- **When:** Friendly consumer apps, wellness, education, onboarding flows
- **Tokens:** Rounded corners (16-24px), dual shadows (outer + inner), pastel palette
- **Anti-patterns:** Looks toy-like for professional/financial products

## Brutalism
Raw, bold, intentionally rough. Thick borders, stark contrast, no polish.
- **When:** Creative agencies, experimental products, developer tools with attitude
- **Tokens:** Harsh borders (2-4px solid), system/mono fonts, black+white+1 neon accent
- **Anti-patterns:** Inaccessible, confusing without strong information hierarchy

## Neomorphism / Soft UI
Extruded/embossed surfaces using light/dark shadows on matching background.
- **When:** Settings panels, single-purpose tools, experimental interfaces
- **Tokens:** Matching bg+surface color, dual box-shadow (light+dark offset), low contrast
- **Anti-patterns:** Poor accessibility (low contrast), hard to show active/focus states

## Dark Mode Native
Designed dark-first (not inverted light mode). Surfaces lighten with elevation.
- **When:** Developer tools, media apps, creative tools, monitoring dashboards
- **Tokens:** Base near-black (#0a0a0a-#1a1a1a), surfaces +3-5% lightness per level, muted accents
- **Anti-patterns:** Pure black (#000) backgrounds, oversaturated colors, harsh white text

## Gradient Mesh
Rich, multi-stop gradients as background elements or accent features.
- **When:** Landing pages, hero sections, brand-heavy pages, creative products
- **Tokens:** Multi-stop radial/conic gradients, mesh-gradient backgrounds, subtle noise overlay
- **Anti-patterns:** Gradients on every surface, illegible text over gradients

## Retro / Pixel
Nostalgic, 8-bit inspired, pixel fonts, limited color palettes.
- **When:** Gaming, indie products, playful consumer apps
- **Tokens:** Pixel fonts, sharp corners (0px radius), limited 4-8 color palette, 1px borders
- **Anti-patterns:** Hard to read at small sizes, unprofessional for B2B

## Organic / Natural
Flowing shapes, natural textures, earth tones, hand-drawn elements.
- **When:** Wellness, food, sustainability, craft products
- **Tokens:** Blob shapes (border-radius: 30% 70%), earth tone palette, textured backgrounds
- **Anti-patterns:** Feels dated without modern typography, hard to maintain consistency

## Luxury / Refined
Premium feel through restraint. Thin typography, gold/champagne accents, generous space.
- **When:** Finance, premium SaaS, fashion, high-end services
- **Tokens:** Thin serif fonts, metallic accent colors, ultra-generous whitespace, subtle animations
- **Anti-patterns:** Slow without purpose, empty rather than spacious

## Industrial / Utilitarian
Dense, information-rich, monospace, tool-like. Form follows function.
- **When:** Trading platforms, analytics dashboards, developer tools, monitoring
- **Tokens:** Monospace fonts, tight spacing (4px base), high density, minimal decoration
- **Anti-patterns:** Overwhelming without clear hierarchy, steep learning curve

---

## Style Selection Reasoning

| Product Category | Primary Style | Secondary Style |
|-----------------|---------------|-----------------|
| SaaS / B2B | Minimalism | Dark Mode Native |
| Consumer / Social | Claymorphism | Glassmorphism |
| Developer Tools | Industrial | Brutalism |
| Creative / Agency | Brutalism | Editorial |
| Finance / Trading | Luxury | Industrial |
| Wellness / Health | Organic | Claymorphism |
| Content / Media | Editorial | Minimalism |
| Landing / Marketing | Gradient Mesh | Editorial |
| Education | Claymorphism | Minimalism |
