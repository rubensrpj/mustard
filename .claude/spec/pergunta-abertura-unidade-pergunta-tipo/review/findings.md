Full suite: `cargo test -p mustard-core -p mustard-rt -p mustard-cli` -> 2841 passed, 4 ignored (53 suites).

## Verdict per claim

**T1/T2 — prose laws (PASS).** dispatch.md renders `sai de` above `tipo`; `tipo` offers exactly `[fix] feature hotfix chore …ou o seu`; the neighbouring rule names the 4-option ceiling, forbids pairing ("cartesian product"), pins `hotfix` (PINNED), and makes `branch` a CORRECTABLE field. All three new ratchets green.

**T3 — operator name outranks the derivation (PASS, verified live).** Real gate run against a real origin:
  --unit-name "Botão de Login Quebrado" -> {"spec":"botao-login-quebrado","branch":"fix/botao-login-quebrado","renamedFrom":"palpite-do-chamador","nameFrom":"operator"}
  without it -> {"spec":"corrigir-botao-login","nameFrom":"derived-from-intent"}
One name, one spelling: the minted `spec` reaches compute_work_branch (emit_pipeline.rs:342 -> work_branch.rs:139), so branch, events and spec dir cannot diverge. `--spec` still loses.

**T4/T5/T6 (PASS).** `cmp` clean on both delivered copies vs seeds. All six AC commands run: AC-1/2/3/5 `1 passed`, AC-4 `1 passed`, AC-6 `2 passed`.

**CHANGE REQUESTS.** CR1 -> AC-2/AC-5. CR2 -> AC-4/AC-5. CR4 (structural split; correct the false comment) -> AC-6; both hooks invoked live against a fresh `mustard init` project: UserPromptSubmit returns 5,852 chars with `## Intent Routing` and no `## Dispatch` leak; SessionStart returns 7,994 chars carrying `# Dispatch Rules`, the `sai de` row and `--unit-name`. template_budget.rs:16-17 now quotes the real doc. Migration backfill_dispatch_inject covers pre-split installs, tested both ways. Nothing silently dropped.

## Findings

**MAJOR — the new ceiling ratchet measures the wrong quantity.** project_seed.rs:1957 and template_budget.rs:40 cap each FILE at 9,500. The binding constraint on sessionStart is the SUM: apps/rt/src/hooks/session/session_start_inject.rs:390 folds [terrain, injected, drift, prune] into ONE additionalContext. dispatch.md at 8,072 chars leaves ~1,900 for a terrain census that scales ~45 chars per subproject. Measured by feeding synthetic grain.model.json to the real hook:

  7 subprojects (this repo) -> ~8,400
  40 -> 9,679
  50 -> 10,079  OVER the ceiling
  60 -> 10,479

dispatch.md sits at index 1 in that fold, so it is exactly what lands in the overflow file. Green ratchet, reachable failure. Not reproducible in this repo, so not blocking — but AC-6's wording ("cada arquivo cabe") certifies per-file where the guarantee needed is per-event.

**MINOR** — spec.md:157-171: T1-T4 and T6 still unchecked while T5 is checked; bookkeeping drift against work that is demonstrably done.

**MINOR** — project_seed.rs:1240: backfill_dispatch_inject gates on an exact-string match of `.claude/mustard/orchestrator.md`. A project that declared it with `./` or a backslash gets dispatch.md seeded to disk and never delivered — the state the function's own doc calls "strictly worse than the over-budget file it replaced".

Build warnings (git_flow.rs, feature.rs, work_kind.rs) are pre-existing; none of those files are in the diff.

No `## Guards` rule is violated. No {role}-pattern skills exist in this subproject, so no mold contract applies.
