# Dashboard Impl Agent Memory

- [Test files excluded from tsconfig](feedback_test_tsconfig_exclude.md) — test files must be excluded from tsconfig.json to avoid breaking the tsc build when vitest/RTL are not yet installed
- [Telemetry DB split](project_dashboard_telemetry_db_split.md) — dashboard reads cost/tokens from separate telemetry.db (run_usage/usage_totals); open TelemetryStore once per cmd outside with_db mutex; two cost sources stay on separate cards
- [Inline-visual checker loophole](project_inline_visual_checker_loophole.md) — AC-10 checker only scans className attributes directly; tokens via `const x = "..."` + template literal pass; `bg-primary/10` inline fails but works via variable; `last:`/`group:` prefixes always fail (only focus/hover/active/disabled recurse)
