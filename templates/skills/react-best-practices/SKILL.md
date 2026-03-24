---
name: react-best-practices
description: Comprehensive React and Next.js performance optimization guide with 40+ rules for eliminating waterfalls, optimizing bundles, and improving rendering. Use when optimizing React apps, reviewing performance, or refactoring components.
---

# React Best Practices - Performance Optimization

40+ rules organized by impact. Use when optimizing React/Next.js apps.

## Critical Priorities

1. **Defer await until needed** — Move awaits into branches where used
2. **Use Promise.all()** — Parallelize independent async operations
3. **Avoid barrel imports** — Import directly from source files
4. **Dynamic imports** — Lazy-load heavy components (`next/dynamic`)
5. **Strategic Suspense** — Stream content while showing layout

## Categories by Impact

| Priority | Category | Key Rules |
|----------|----------|-----------|
| CRITICAL | Waterfalls | Defer await, Promise.all(), Suspense boundaries |
| CRITICAL | Bundle Size | Direct imports, conditional loading, dynamic imports |
| HIGH | Server-Side | LRU caching, minimize RSC serialization, React.cache() |
| MEDIUM-HIGH | Client Data | SWR deduplication, deduplicate event listeners |
| MEDIUM | Re-renders | Defer state reads, memoize components, narrow effect deps |
| MEDIUM | Rendering | content-visibility, hoist static JSX, Activity component |
| LOW-MEDIUM | JS Perf | Set/Map O(1) lookups, batch DOM, hoist RegExp, toSorted() |

## Common Anti-Patterns

- Barrel imports from large libraries → import directly
- Sequential awaits → Promise.all()
- Re-render entire trees → memoize subtrees
- Analytics in critical path → lazy-load
- .sort() mutation → .toSorted()
- RegExp/objects inside render → hoist outside

## Approach

1. Profile first (React DevTools + browser perf)
2. Focus on critical paths (waterfalls + bundle)
3. Measure impact (LCP, TTI, FID)
4. Apply incrementally, test thoroughly
