# Review verdict — spec fase-1-pilar-2-2a, subproject apps/dashboard

Verdict: APPROVED — 0 critical.

## Acceptance Criteria — all ran, real output
- AC-1 `cargo test -p mustard-core -- economy_time_window` → 4 passed. PASS
- AC-2 `-- economy_time_window_absent` → 2 passed. PASS
- AC-3 `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml -- economy_window` → 1 passed; broader economy filter → 2 passed (incl. dashboard_prompt_economy_filters_by_period). PASS
- AC-4 `pnpm --dir apps/dashboard build` (tsc -b && vite build) → ✓ built in 20.08s — real type-check passed. PASS
- AC-5 `cargo build --workspace` → 0 crates to compile, no errors. PASS

## Guards (apps/dashboard/CLAUDE.md) — all satisfied
- invoke() confined to lib/dashboard.ts (withWindow→windowedScope→invoke); WindowBar/Economia.tsx consume wrappers/hooks only.
- Wire keys kind/window/inner/from/to single-word (no camelCase/snake_case break); no rename of repoPath/spec.
- useEconomySummary stable-array key + enabled: !!scope + cast-in-queryFn; period added as stable leaf (not the dayjs window).
- No new page/key-prefix → guard #4 untriggered; period change refetches via queryKey.

## Molds — dashboard-tauri-pattern (fetchEconomy* wraps invoke<T>) followed; dashboard-use-pattern (useQuery stable key + enabled gate) followed; dashboard-detail-pattern N/A (WindowBar is a features/ component mirroring ScopeBar).

## Decisions respected — core Windowed{window,inner} wrapper + into_parts() peel; dashboard_prompt_economy self-folds via value_in_window mirroring core; FE separate EconomyScopeWire; query keys on period.

## Minor observations (non-blocking)
- dashboard_prompt_economy gained backend window support + test but isn't wired to WindowBar on the frontend (binding fetchPromptEconomy passes {repoPath}, pre-existing); backend uniformity harmless/tested.
- RealConsumption block (dashboard_consumption) stays un-windowed — already scope-independent by prior design, matching the plan's file scope.

<VERDICT>{"verdict":"approved","critical":0}</VERDICT>
