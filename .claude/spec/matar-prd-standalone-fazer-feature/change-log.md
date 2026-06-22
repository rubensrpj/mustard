# Change Log — matar-prd-standalone-fazer-feature

_Solicitações registradas automaticamente durante o pipeline (mid-spec). O `spec.md` (narrativa congelada) NÃO é alterado; dobre o que muda comportamento em `## Acceptance Criteria` e rode o QA de novo._

- **2026-06-22T18:07:13.946Z** _(Plan)_ — ar
- **2026-06-22T18:41:50.787Z** _(Execute)_ — vamos deixar esse prd fora do dashboard, não ficou prático e a interação ficou ruim, o que podemos ter no dashboard é um atalho ao prd vinculado a spec. Analise
- **2026-06-22T18:46:22.884Z** _(Execute)_ — aba, vamos seguir sua recomendação.
- **2026-06-22T18:57:24.635Z** _(Execute)_ — Antes do review, está muito lento ao entrar na rota specs isso já havia sido revolvido

## Resolução das solicitações

- **18:41 + 18:46 (PRD fora do dashboard → aba PRD vinculada à spec)** — ATENDIDA. A onda 3 foi redesenhada: a rota/funil de autoria `/prd` saiu por completo e entrou uma aba "PRD" read-only no `SpecDrillDown` (corte `<!-- PRD -->`..`<!-- PLAN -->` via `slicePrdSection`). Dobrada no **AC-6** e re-verificada pelos reviews (PASS).
- **18:57 (lentidão ao entrar em /specs)** — **DISPENSADA desta spec por decisão explícita do usuário** (AskUserQuestion 2026-06-22: "Fechar o PRD, depois /bugfix da perf"). Diagnóstico: a lentidão é **transitória** (storm de invalidação durante a run ativa), **não é regressão do fix anterior** (cache/snapshot intactos — `lib.rs:2107-2121`, `telemetry.rs:1806`) nem efeito da onda 3 (a lista é batchada e o `SpecRow` é puro). NÃO é objetivo desta spec e NÃO foi dobrada em AC — será tratada num `/bugfix` dedicado com profiling. Esta dispensa é o waiver que o review pediu para o close-gate.- **2026-06-22T19:23:14.531Z** _(Execute)_ — <task-notification> <task-id>b9ehiq4eh</task-id> <tool-use-id>toolu_01R9atRnjZmQfnr7ZfdpEmMx</tool-use-id> <output-file>C:\Users\ruben\AppData\Local\Temp\claude\C--Atiz-mustard\7aed5749-8cde-4f5b-983e-44e7edb215da\tasks\b9ehiq4eh.output</output-file> <status>completed</status> <summary>Background command "rtk mustard-rt run close-pipeline --spec matar-prd-standalone-fazer-feature 2&gt;&amp;1 | tail -60" completed (exit code 0)</summary> </task-notification>
