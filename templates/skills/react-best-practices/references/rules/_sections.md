# Sections

This file defines all sections, their ordering, impact levels, and descriptions.
Individual rules are consolidated into category files (one file per section).

---

## 1. Eliminating Waterfalls → `async.md`

**Impact:** CRITICAL
**Description:** Waterfalls are the #1 performance killer. Each sequential await adds full network latency. Eliminating them yields the largest gains.

Rules: `async-api-routes`, `async-defer-await`, `async-dependencies`, `async-parallel`, `async-suspense-boundaries`

## 2. Bundle Size Optimization → `bundle.md`

**Impact:** CRITICAL
**Description:** Reducing initial bundle size improves Time to Interactive and Largest Contentful Paint.

Rules: `bundle-barrel-imports`, `bundle-conditional`, `bundle-defer-third-party`, `bundle-dynamic-imports`, `bundle-preload`

## 3. Server-Side Performance → `server.md`

**Impact:** HIGH
**Description:** Optimizing server-side rendering and data fetching eliminates server-side waterfalls and reduces response times.

Rules: `server-cache-lru`, `server-cache-react`, `server-parallel-fetching`, `server-serialization`

## 4. Client-Side Data Fetching → `client.md`

**Impact:** MEDIUM-HIGH
**Description:** Automatic deduplication and efficient data fetching patterns reduce redundant network requests.

Rules: `client-event-listeners`, `client-swr-dedup`

## 5. Re-render Optimization → `rerender.md`

**Impact:** MEDIUM
**Description:** Reducing unnecessary re-renders minimizes wasted computation and improves UI responsiveness.

Rules: `rerender-defer-reads`, `rerender-dependencies`, `rerender-derived-state`, `rerender-lazy-state-init`, `rerender-memo`, `rerender-transitions`

## 6. Rendering Performance → `rendering.md`

**Impact:** MEDIUM
**Description:** Optimizing the rendering process reduces the work the browser needs to do.

Rules: `rendering-activity`, `rendering-animate-svg-wrapper`, `rendering-conditional-render`, `rendering-content-visibility`, `rendering-hoist-jsx`, `rendering-hydration-no-flicker`, `rendering-svg-precision`

## 7. JavaScript Performance → `js.md`

**Impact:** LOW-MEDIUM
**Description:** Micro-optimizations for hot paths can add up to meaningful improvements.

Rules: `js-batch-dom-css`, `js-cache-function-results`, `js-cache-property-access`, `js-cache-storage`, `js-combine-iterations`, `js-early-exit`, `js-hoist-regexp`, `js-index-maps`, `js-length-check-first`, `js-min-max-loop`, `js-set-map-lookups`, `js-tosorted-immutable`

## 8. Advanced Patterns → `advanced.md`

**Impact:** LOW
**Description:** Advanced patterns for specific cases that require careful implementation.

Rules: `advanced-event-handler-refs`, `advanced-use-latest`
