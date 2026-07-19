---
id: wave.field-report-fix-package-sialia.4-trace
---

# wave-4-trace

## Summary

wave-scaffold traceability also covers the PARENT spec's Acceptance Criteria and gates on gaps

## Network

- Parent: [[spec.field-report-fix-package-sialia]]

## Tasks

- [ ] In wave_scaffold.rs traceability_gaps: extend the `defined` id set with the parent spec.md `## Acceptance Criteria` ids, reusing the shared qa_run extract/parse helpers (no forked parser)
- [ ] Introduce MUSTARD_TRACE_GATE_MODE (strict|warn|off, default strict, same parsing convention as the other gate envs): strict -> an uncovered parent AC makes wave-scaffold exit non-zero listing every gap; warn -> current stderr WARN; off -> silent
- [ ] Keep Gap 1 (wave with tasks but satisfies nothing) WARN-level in every mode — only the parent-AC coverage gap escalates
- [ ] Unit tests named with `parent_spec_ac`: a parent AC id absent from every wave's satisfies/acceptance fires the gap; strict blocks (non-zero); warn passes with stderr; ids covered via acceptance lines still count as covered

## Files

- `apps/rt/src/commands/wave/wave_scaffold.rs`
