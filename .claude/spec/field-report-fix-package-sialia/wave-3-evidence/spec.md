---
id: wave.field-report-fix-package-sialia.3-evidence
---

# wave-3-evidence

## Summary

qa-run gains an opt-in Expect: evidence regex per AC — exit 0 alone no longer proves a declared-evidence AC

## Network

- Parent: [[spec.field-report-fix-package-sialia]]

## Tasks

- [ ] Add `regex = "1"` inline to apps/rt/Cargo.toml dependencies (mirrors apps/scan; NOT yet a workspace dep) — needed for the Expect: matcher
- [ ] In qa_run/mod.rs add expect: Option<String> to AcItem and parse an `Expect:` line (backtick-wrapped regex) with the same lookahead style extract_command uses for `Command:`
- [ ] In runner.rs run_ac_command: when status.success() AND item declares expect, regex-match the combined stdout+stderr; no match -> status fail with a stderr_excerpt stating expected evidence was not found (include the pattern and an output excerpt); match -> pass; no Expect: declared -> legacy exit-code behavior byte-for-byte
- [ ] Invalid regex in Expect: -> status skip with a reason (fail-open, never a panic); keep the pure matching logic in a small testable function (SRP)
- [ ] In analyze_validation.rs extend the weak-AC lint: a test-shaped Command (the existing is_weak_ac_command vocabulary for test runners) WITHOUT an Expect: line gets a WARN suggesting a declared evidence regex; keep it WARN-level and language-agnostic (no runner-specific output parsing)
- [ ] Unit tests named with `expect_regex`: exit0+no-match fails, exit0+match passes, absent Expect keeps legacy pass, invalid regex skips, weak-AC lint fires on test command without Expect

## Files

- `apps/rt/src/commands/review/qa_run/mod.rs`
- `apps/rt/src/commands/review/qa_run/runner.rs`
- `apps/rt/src/commands/review/analyze_validation.rs`
