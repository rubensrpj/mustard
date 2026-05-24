# Wave Plan — Wave network como padrão Mustard

## PRD (visão única)

Eleva a quebra em waves de "opt-in via signals frágeis" para característica **padrão** do Mustard, com seis peças amarradas como uma só (SDD canônico: cada fase é artefato primeiro-classe em arquivo próprio):

1. **Auto-decomposição em arquivos**: `/mustard:feature` Full scope com sinais reais de dependência (`file_count≥6` OR `layer_count≥3` OR `independent_subbehaviors≥3`) gera `wave-plan.md` + `wave-N-{role}/spec.md` automaticamente, sem AskUserQuestion. Single `spec.md` permanece só para Light.
2. **Review e QA também como arquivos**: junto dos wave-files, scaffold também cria `review/spec.md` e `qa/spec.md` no parent dir. `review/spec.md` declara checklist por categoria + onde vai parar o verdict (`review/verdict.md` após execução). `qa/spec.md` consolida os AC de todas as waves + onde vai o relatório (`qa/report.md`). Hoje review fica inline em `## Concerns` e QA em `.claude/.qa-reports/{spec}.json` — fora do dir da spec, sem padrão de plano-antes-de-rodar. Equipará SDD: você lê o plano de QA antes da execução, não só o relatório depois.
3. **Cross-wave shared memory**: o agente da wave N recebe no prompt um bloco `## Memórias de waves anteriores` resumindo o que cada agente das waves 1..N-1 gravou via `mustard-rt run memory agent` (hoje só grava, falta o read-side injetado). Implementado em novo subcomando `mustard-rt run memory cross-wave --spec --wave N`.
4. **Wikilinks Obsidian-style**: `[[parent-spec]]`, `[[wave-N-role]]`, `[[review]]`, `[[qa]]` viram sintaxe reconhecida em campos `Parent:`, dependências, e nova seção `## Network`. Parser do dashboard extrai e renderiza como grafo navegável (aba "Network" em `SpecDrillDown`), mesmo princípio que [[2026-05-20-tactical-fix-via-sub-spec]] já estabelece via `spec.link`.
5. **Métricas funcionais agrupadas por parent**: novo subcomando `mustard-rt run metrics wave-status --spec <parent>` devolve agregação por wave (status atual, tokens economizados, duração, retries, tamanho da memória cross-wave injetada). Dashboard renderiza TODAS as métricas (Economia, Quality, Activity, Telemetry) com a hierarquia parent→waves preservada — nunca soma cega entre specs irrelacionadas. Inclui diagnose + fix da área de métrica atual que o operador reporta quebrada (RTK savings, token.saved aggregation, cross-wave parse).
6. **Orquestrador define modelo, agente nunca escolhe**: a coluna `Modelo` no wave-plan.md é fonte de verdade — preenchida pela SKILL `/feature` durante PLAN (ou ajustada manualmente pelo operador antes do approve). SKILL `/resume` lê dela ao dispatchar cada wave. `model_routing` module continua bloqueando upgrades vs routing table, mas a escolha primária vem do wave-plan, não do agente.

A motivação é eliminar 6 incoerências atuais: (a) wave inline em spec.md monolítico fere a uniformidade "uma fase = um arquivo", impede progresso por wave no dashboard e sufoca o read humano; (b) review e QA não têm plano declarado upfront — só relatórios pós-fato em locais não-uniformes; (c) memória de agente só write-side é promessa quebrada — wave seguinte não aprende nada com a anterior; (d) relações entre specs (parent, children, dependências, review, qa) hoje vivem em texto plano impossível de navegar no dashboard; (e) métricas atuais somam cego e algumas estão quebradas (operador não confia em nenhum KPI); (f) escolha de modelo está difusa entre routing, agente e hooks — operador não tem governança clara.

## Métrica de sucesso

- Rodar `/mustard:feature <name>` com sinais Full+deps gera wave-files sem perguntar nada.
- Dispatch da wave N inclui no prompt do agente o bloco de memórias das waves 1..N-1.
- Spec.md (parent e wave files) com `[[name]]` renderiza link clicável na aba "Network" do `SpecDrillDown`, com grafo visual mostrando todas as conexões da spec corrente.

## Não-Objetivos globais

- Não migrar specs históricas (completed/) para wave-files.
- Não tocar Light scope (single spec.md continua o default lá).
- Não criar UI de edição de wikilinks — só leitura/renderização.
- Não substituir `mustard-rt run spec-link` (parent↔child explícito) — wikilinks são complemento descritivo.
- Não criar persistência Obsidian-compatible explícita (já é texto markdown puro, Obsidian renderiza nativo).
- Não tocar QA/Review como arquivo (assunto da próxima evolução, fora do escopo).

## Tabela de Waves

| Wave | Spec                            | Role     | Modelo (decidido pelo orquestrador) | Status   | Depende de              | Resumo                                                                |
|------|---------------------------------|----------|-------------------------------------|----------|-------------------------|-----------------------------------------------------------------------|
| 1    | [[wave-1-rt-infra]]             | general  | opus                                | completed | —                      | `mustard-rt` ganha 4 subcomandos (`wikilink-extract`, `memory cross-wave`, `wave-scaffold`, `metrics wave-status`) + tabela `wikilinks` no SQLite |
| 2    | [[wave-2-skill-template]]       | general  | opus                                | completed    | [[wave-1-rt-infra]]    | SKILLs `/feature` e `/resume` chamam `wave-scaffold` + `memory cross-wave`; agent-prompt template ganha `{cross_wave_memory}`; modelo lido do wave-plan |
| 3    | [[wave-3-dashboard-graph]]      | frontend | opus                                | implementing | [[wave-1-rt-infra]]    | `SpecDrillDown` ganha aba "Network" renderizando grafo wikilink + memórias por wave + nós `review`/`qa` |
| 4    | [[wave-4-metrics-diagnose-fix]] | general  | opus                                | completed    | [[wave-1-rt-infra]]    | Diagnose das métricas quebradas (RTK, token.saved, cross-wave parse) + fix + agrupamento por parent em todas as queries; dashboard renderiza KPIs em tree |

Além das waves de execução, esta spec entrega — **demonstrando o padrão** — os artefatos padrão SDD:

| Plano    | Arquivo               | Conteúdo                                                              |
|----------|------------------------|------------------------------------------------------------------------|
| Review   | [[review]] (`review/spec.md`)  | Checklist por categoria, agente designado, onde grava `review/verdict.md` |
| QA       | [[qa]] (`qa/spec.md`)          | Consolidação dos AC globais + cada wave, comando do qa-runner, onde grava `qa/report.md` |

**Paralelismo:** [[wave-2-skill-template]], [[wave-3-dashboard-graph]] e [[wave-4-metrics-diagnose-fix]] são todas paralelizáveis depois de [[wave-1-rt-infra]] (nenhuma compartilha arquivos com outra).

## Network

- Children (execução): [[wave-1-rt-infra]], [[wave-2-skill-template]], [[wave-3-dashboard-graph]], [[wave-4-metrics-diagnose-fix]]
- Planos SDD (declarados upfront, executados ao final): [[review]], [[qa]]
- Spec irmã que demonstra o padrão manualmente (consumidora natural assim que esta entregar): [[2026-05-20-dashboard-visual-overview]]
- Spec irmã com mesmo princípio (sub-spec linkada como rastreabilidade): [[2026-05-20-tactical-fix-via-sub-spec]]
- Sub-spec gerada por discovery em wave-4 (Audit-1 deferred): [[2026-05-20-metrics-writers-pipeline-key]]

## Critérios de Aceitação globais

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-G1: `mustard-rt run wikilink-extract` expõe a flag `--spec-dir` no help — Command: `bash -c 'mustard-rt run wikilink-extract --help 2>&1 | grep -q -- "--spec-dir"'`
- [x] AC-G2: `mustard-rt run memory cross-wave` expõe as flags `--spec` e `--wave` no help — Command: `node -e "const o=require('child_process').execSync('mustard-rt run memory cross-wave --help').toString();if(!o.includes('--spec')||!o.includes('--wave'))throw new Error('flags missing')"`
- [x] AC-G3: SKILL `/feature` força wave-files (texto contém regra explícita) — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/feature/SKILL.md','utf8');if(!/wave-scaffold.*OBRIGAT|OBRIGAT.*wave-scaffold/i.test(t))throw new Error('wave-scaffold enforcement missing in SKILL')"`
- [x] AC-G4: Cargo check passa em mustard-rt — Command: `cargo check -p mustard-rt`
- [x] AC-G5: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-G6: `metrics wave-status` expõe a flag `--spec` — Command: `bash -c 'mustard-rt run metrics wave-status --help 2>&1 | grep -q -- "--spec"'`
- [x] AC-G7: SKILL `/resume` lê `Modelo` do wave-plan ao dispatchar — Command: `node -e "const t=require('fs').readFileSync('apps/cli/templates/commands/mustard/resume/SKILL.md','utf8');if(!/wave-plan.*Modelo|Modelo.*wave-plan/.test(t))throw new Error('SKILL resume does not link wave-plan and Modelo')"`

## Limites globais

```
ESCOPO:
  apps/rt/src/run/wikilink.rs                  (new)
  apps/rt/src/run/memory_cross_wave.rs         (new)
  apps/rt/src/run/wave_scaffold.rs             (new — scaffolda wave-N + review/ + qa/)
  apps/rt/src/run/metrics_wave_status.rs       (new — agregação por wave agrupada por parent)
  apps/rt/src/run/mod.rs                       (modify — register subcommands)
  apps/cli/templates/commands/mustard/feature/SKILL.md   (modify)
  apps/cli/templates/commands/mustard/resume/SKILL.md    (modify — cross-wave + modelo do wave-plan)
  apps/cli/templates/refs/agent-prompt/agent-prompt.md   (modify)
  apps/dashboard/src/components/specs/SpecNetworkTab.tsx (new)
  apps/dashboard/src/components/specs/SpecDrillDown.tsx  (modify — add tab + parent-grouping)
  apps/dashboard/src/components/economia/**              (modify — group by parent)
  apps/dashboard/src/pages/Economia.tsx                  (modify — tree view por parent)
  apps/dashboard/src/lib/dashboard.ts                    (modify — invoke wrappers)
  apps/dashboard/src-tauri/src/spec_views.rs             (modify — bridge subcomandos)
  apps/dashboard/src-tauri/src/main.rs                   (modify — register handlers)
  packages/core/src/store/wikilinks.rs                   (new — schema + CRUD)

OUT-OF-BOUNDS:
  Light scope flow (continua single spec.md)
  Specs históricas (completed/)
  QA/Review file format (próxima evolução)
  Sidebar, Topbar, outras pages do dashboard
```
