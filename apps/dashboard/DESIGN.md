## Mustard Overrides

This pack was sourced from the Binance design system (getdesign binance pack) and applied
with the following deliberate overrides for the Mustard product:

1. **Brand color**: `--primary` is preserved as Mustard yellow `#dfab01` (light) /
   `#e6c84a` (dark) instead of Binance yellow `#FCD535`. The CTA contrast model
   (black text on yellow background) is kept — only the hue shifts toward amber.

2. **Typography**: BinanceNova (display) and BinancePlex (mono) are proprietary
   Binance typefaces without a public license. Substitutes used:
   - Display/body → **Inter Variable** (`@fontsource-variable/inter`) — already loaded
   - Monospace → **IBM Plex Mono Variable** (`@fontsource-variable/ibm-plex-mono`) —
     spirit match to BinancePlex (both IBM geometry, similar x-height)

3. **Lifecycle phase tokens preserved**: `--color-phase-analyze`, `--color-phase-plan`,
   `--color-phase-execute`, `--color-phase-qa`, `--color-phase-backlog`,
   `--color-phase-close`, and `--color-event-fail` are NOT trading colors — they encode
   Mustard pipeline lifecycle states and are kept unchanged across both themes.

4. **All other tokens adopted integrally from Binance**: canvas `#0b0e11`, card
   `#1e2329`, card-elevated `#2b3139`, hairline `#eaecef`, secondary text `#848e9c`,
   up/green `#0ecb81`, down/red `#f6465d`, amber `#f0b90b`, blue `#1e88e5`.
   Update this file via `npx getdesign add binance --force` when the pack updates.

---

# Binance Design System

Source: getdesign binance pack · applied 2026-05-23

## Color Palette

### Canvas & Surfaces

| Token | Light | Dark | Usage |
|-------|-------|------|-------|
| Background (canvas) | `#ffffff` | `#0b0e11` | Page background |
| Card | `#ffffff` | `#1e2329` | Card, panel surfaces |
| Card-elevated | `#f5f5f5` | `#2b3139` | Elevated card, muted bg |
| Foreground | `#1e2329` | `#eaecef` | Primary text |
| Secondary text | `#848e9c` | `#848e9c` | Muted / secondary text |

### Brand

| Token | Value | Usage |
|-------|-------|-------|
| Primary (Mustard override) | `#dfab01` / dark `#e6c84a` | CTAs, ring, active states |
| Primary foreground | `#000000` | Text on primary buttons |
| Binance yellow (original) | `#FCD535` | Reference only — not used |

### Trading Direction

| Token | Value | Usage |
|-------|-------|-------|
| Up / success | `#0ecb81` | Positive delta, success intent |
| Down / danger | `#f6465d` | Negative delta, error intent |
| Amber / warning | `#f0b90b` | Warning intent |
| Blue / info | `#1e88e5` | Info intent |

### Hairline & Borders

| Token | Light | Dark | Usage |
|-------|-------|------|-------|
| Border / hairline | `#eaecef` | `#2b3139` | Dividers, borders, inputs |

## Typography

| Token | Value | Notes |
|-------|-------|-------|
| `--font-sans` | Inter Variable, ui-sans-serif, system-ui | Replaces BinanceNova (proprietary) |
| `--font-mono` | IBM Plex Mono Variable, ui-monospace | Replaces BinancePlex (proprietary) |

## Spacing & Shape

| Token | Value | Notes |
|-------|-------|-------|
| `--radius` | `6px` | Button radius (Binance standard) |
| `--radius-card` | `8px` | Card radius |
| `--editorial-band-py` | `80px` | Editorial hero section vertical padding |

## Elevation

Binance uses flat surfaces with `#2b3139` dividers in dark mode rather than
drop shadows. Light mode uses subtle `rgba(0,0,0,0.08)` shadows.

## Component Patterns

### CTA Button
- Background: `--primary` (#dfab01 Mustard / #FCD535 Binance reference)
- Text: `--primary-foreground` (#000000 — black on yellow, maximum contrast)
- Border-radius: `--radius` (6px)
- Hover: `--primary-pressed` (darkened variant)

### Card
- Background: `--card` (#1e2329 dark)
- Border: `1px solid --border` (#2b3139 dark)
- Border-radius: `--radius-card` (8px)

### Input
- Background: `--input` (#2b3139 dark)
- Border: `1px solid --border`
- Focus ring: `--ring` (primary color)

### Status indicators
- Success / up: `--intent-success` (#0ecb81)
- Error / down: `--intent-error` (#f6465d)
- Warning: `--intent-warning` (#f0b90b)
- Info: `--intent-info` (#1e88e5)
