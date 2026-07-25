---
id: wave.structured-review-verdict-capture-via.plan
---

# Wave Plan

## Wave Table

| Wave | Spec | Role | Depends on | Summary |
|------|------|------|------------|---------|
| 1 | [[wave.structured-review-verdict-capture-via.1-contract]] | contract | — | Emit the structured <VERDICT> block from the review agent |
| 2 | [[wave.structured-review-verdict-capture-via.2-capture]] | capture | [[wave.structured-review-verdict-capture-via.1-contract]] | Parse <VERDICT> on SubagentStop and auto-emit review.result, mirroring the <MEMORY> capture |

## Acceptance Criteria
- AC-1 — when a review subagent emits a <VERDICT> block, then the SubagentStop hook emits review.result with matching verdict/criticalCount. Command: `cargo test -p mustard-rt capture_review_verdict_emits_review_result` Expect: `1 passed; 0 failed`
- AC-2 — when no/malformed <VERDICT> block, then the hook is a silent no-op (fail-open). Command: `cargo test -p mustard-rt verdict_block_absent_is_noop` Expect: `1 passed; 0 failed`
- AC-3 — the project build and tests pass green. Command: `cargo build --workspace`
