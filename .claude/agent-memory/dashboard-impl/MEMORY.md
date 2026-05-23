# Dashboard Impl Agent Memory

- [Test files excluded from tsconfig](feedback_test_tsconfig_exclude.md) — test files must be excluded from tsconfig.json to avoid breaking the tsc build when vitest/RTL are not yet installed
- [Telemetry DB split](project_dashboard_telemetry_db_split.md) — dashboard reads cost/tokens from separate telemetry.db (run_usage/usage_totals); open TelemetryStore once per cmd outside with_db mutex; two cost sources stay on separate cards
