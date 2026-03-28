# UX Guidelines by Priority

Rules organized by criticality. Higher priority = harder failures when violated.

---

## CRITICAL — Accessibility

- **Contrast ratios:** 4.5:1 minimum for body text, 3:1 for large text (18px+ or 14px bold+)
- **Keyboard navigation:** All interactive elements reachable via Tab, activatable via Enter/Space
- **Focus indicators:** Visible focus ring on every focusable element — never `outline: none` without replacement
- **Alt text:** Every `<img>` has descriptive alt text; decorative images use `alt=""`
- **ARIA labels:** Icons-only buttons must have `aria-label`; custom controls need proper roles
- **Color independence:** Never use color alone to convey meaning — add icons, text, or patterns
- **Reduced motion:** Respect `prefers-reduced-motion` — disable non-essential animations

## CRITICAL — Touch & Interaction

- **Touch targets:** Minimum 44x44px for all interactive elements (buttons, links, checkboxes)
- **Target spacing:** Minimum 8px between adjacent touch targets
- **Feedback timing:** Visual feedback within 100ms of interaction
- **Scroll behavior:** No horizontal scroll on mobile viewports; `scroll-behavior: smooth` for anchor links
- **Double-tap prevention:** Buttons disabled after submit, forms prevent double submission

## HIGH — Performance

- **Image optimization:** WebP/AVIF with fallbacks, `loading="lazy"` for below-fold images
- **Layout shift:** Set explicit `width` and `height` on images/iframes to prevent CLS
- **Font loading:** `font-display: swap` for web fonts; preload critical fonts
- **CSS containment:** Use `contain: layout` on complex components to isolate reflows
- **Critical CSS:** Inline above-fold styles, defer non-critical CSS

## HIGH — Layout & Responsive

- **Mobile-first:** Design for 320px minimum, scale up
- **Viewport meta:** `<meta name="viewport" content="width=device-width, initial-scale=1">`
- **Breakpoints:** Use content-driven breakpoints, not device-specific
- **No horizontal overflow:** Test every page at 320px — nothing should overflow
- **Safe areas:** Respect `env(safe-area-inset-*)` for notched devices

## HIGH — Navigation Patterns

- **Bottom nav items:** Maximum 5 for mobile bottom navigation
- **Deep linking:** Every significant view should have a unique URL/route
- **Back button consistency:** Browser/device back always works as expected
- **Loading states:** Show skeleton/placeholder during data fetch, never blank screens
- **Error states:** Every data view needs empty, loading, and error states

## MEDIUM — Typography & Color

- **Semantic color tokens:** Use design tokens, not hardcoded hex values
- **Line height:** Body text at 1.5 minimum for readability
- **Paragraph width:** Maximum 65-75 characters per line for comfortable reading
- **Font sizes:** Minimum 16px for body text on mobile (prevents iOS zoom)
- **Color consistency:** Same semantic meaning = same color throughout the app

## MEDIUM — Animation & Motion

- **Duration range:** 150-300ms for UI transitions; longer feels sluggish
- **Easing:** Use ease-out for entries, ease-in for exits; never linear for UI
- **Purpose:** Every animation must have a purpose (guide attention, show relationship, provide feedback)
- **Reduced motion:** Provide `prefers-reduced-motion` alternatives for all animations
- **No infinite loops:** Avoid perpetual spinning/pulsing except for loading indicators

## MEDIUM — Forms & Feedback

- **Visible labels:** Every input has a visible label (not just placeholder)
- **Error placement:** Inline errors below the field, not just top-of-form alerts
- **Required fields:** Mark required fields clearly; don't rely on asterisk alone
- **Progressive disclosure:** Show advanced options only when needed
- **Auto-focus:** First input auto-focused on form pages; focus trapped in modals

## LOW — Charts & Data Visualization

- **Accessible colors:** Colorblind-safe palettes; don't rely on color alone for data series
- **Legends:** Always include legends for multi-series charts
- **Tooltips:** Interactive data points show values on hover/tap
- **Screen reader:** Provide text summary of chart data for accessibility
- **Responsive charts:** Charts resize with container; labels don't overlap at small sizes
