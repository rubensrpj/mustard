---
id: wave.structured-review-verdict-capture-via.2-capture
---

# wave-2-capture

## Summary

Parse <VERDICT> on SubagentStop and auto-emit review.result, mirroring the <MEMORY> capture

## Network

- Parent: [[spec.structured-review-verdict-capture-via]]
- Depends on: [[wave.structured-review-verdict-capture-via.1-contract]]

## Tasks

- [ ] In apps/rt/src/hooks/task/subagent_inject.rs add extract_verdict_block + capture_review_verdict[_with_session] mirroring the <MEMORY> twins; on the review role emit review.result via review_result::record_review; fail-open (no block / wrong role / parse error -> no-op); wire next to capture_memory_decision in the SubagentStop handler
- [ ] Reuse record_review in apps/rt/src/commands/review/review_result.rs to emit the review.result event + review metric from the parsed verdict; keep the manual CLI path intact
- [ ] Add tests mirroring the capture_memory_decision tests: AC-1 (block parsed -> event emitted with matching fields) and AC-2 (absent/malformed -> no-op)

## Files

- `apps/rt/src/hooks/task/subagent_inject.rs`
- `apps/rt/src/commands/review/review_result.rs`
