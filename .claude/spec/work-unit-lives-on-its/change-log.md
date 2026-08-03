# Change Log — work-unit-lives-on-its

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-07-31T21:24:20.054Z** _(Execute)_ — segue
- **2026-08-03T09:34:21.853Z** _(Execute)_ — mas isso é agnóstico ou ficará hardcoded no código?
- **2026-08-03T09:39:47.510Z** _(Execute)_ — pergunta final qual versão é melhor o que foi feito será perdido, não estou sabendo decidir
- **2026-08-03T09:41:58.534Z** _(Execute)_ — faça aqui mesmo
- **2026-08-03T10:17:25.388Z** _(Execute)_ — deixe tudo aqui, faça pr e merge até o main e apague após o merge esses branchs
- **2026-08-03T10:23:01.839Z** _(QaReview)_ — sim
- **2026-08-03T10:24:43.682Z** _(QaReview)_ — faça tudo
- **2026-08-03T10:52:48.803Z** _(QaReview)_ — **Instruction:** The EXECUTE isolation step must degrade instead of failing. Since spec-draft now checks the work branch out in the MAIN checkout at PLAN, work_unit_open::open_at must detect that the requested branch is already checked out there and report the unit as already isolated in place (ok:true, inPlace:true) rather than attempting a git worktree add that git refuses with exit 128. A worktree is still cut when the branch is NOT already checked out, so parallel work on several units keeps working. Both prose copies (packages/core/templates/mustard/orchestrator.md and .claude/mustard/orchestrator.md) plus plugin/refs/git/git-flow.md must state the new truth: the branch IS the isolation, cut at approval, and the worktree is the parallel-work case rather than the default step.
- **2026-08-03T10:52:48.901Z** _(QaReview)_ — **Instruction:** pr-review runs from an integration base by design, but this spec moved the spec directory onto the work branch, so the spec is not present in the base checkout at all. Reading it via main_checkout_root or via the current checkout both return nothing. pr-review must read the spec out of the PR's OWN branch (for example git show <headRefName>:.claude/spec/<slug>/spec.md) so that spec_path, subproject and patterns are populated exactly as pr.md promises, and the recorded review.result must land where the unit can see it rather than in a base checkout that does not track it.
- **2026-08-03T10:59:22.731Z** _(QaReview)_ — existe algo pendente?
- **2026-08-03T11:01:25.503Z** _(QaReview)_ — sim faça
