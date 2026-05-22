# Review — economia-moat-unification

### Parent: [[2026-05-20-economia-moat-unification]]
### Stage: QaReview
### Outcome: Active
### Flags: 
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T06:15:00Z
### Lang: pt

## PRD

Auditar as 8 waves entregues contra a checklist canônica do Mustard (SOLID, Design System, Patterns, i18n, Integration, Build, Elegance). 3 reviewers paralelos (1 por subprojeto afetado): `mustard-core`, `mustard-rt`, `mustard-dashboard`. Cada reviewer classifica achados em CRITICAL / WARNING / NOTE. Verdict consolidado em `verdict.md`.

## Subprojetos afetados

| Reviewer | Subproject | Waves cobertas | Files |
|----------|------------|----------------|-------|
| core-reviewer | `packages/core/` | W1, W3a, W4 | economy/{model,scope,writer,reader,estimator,multi_project,store,sources/{otel,transcript,rtk,mod}}.rs, store/migrations.rs, tests/economy_{basic,attribution}.rs, lib.rs, Cargo.toml |
| rt-reviewer | `apps/rt/` | W2, W3b | hooks/{bash_guard,model_routing,budget,tracker,session_start,session_cleanup}.rs, run/{rtk_gain,transcript_watcher,otel/collector,spec_extract}.rs, run/mod.rs, main.rs, Cargo.toml |
| dashboard-reviewer | `apps/dashboard/` | W5, W6, W7, W8 | styles/theme.css, components/{ds,trace,economy,workspace}/*, hooks/{useSpecTrace,useEconomySummary}.ts, lib/{i18n,types/{trace,economy},dashboard}.ts, pages/{Workspace,Economia}.tsx, src-tauri/src/{telemetry,lib,spec_views}.rs, src-tauri/tests/top_files_today_test.rs, NOTICE.md |

## Checklist (7 categorias)

Cada reviewer responde por categoria. Severidades:
- **CRITICAL**: bloqueia CLOSE (mau funcionamento, vazamento, design quebrado, segurança)
- **WARNING**: recomendado consertar (code smell sério, inconsistência, falta de teste)
- **NOTE**: sugestão (cosmético, melhor naming, otimização opcional)

1. **SOLID**: separação de responsabilidades, ausência de deus-classe, deps invertidas onde fizer sentido
2. **Design System** (só dashboard): primitivas DS usadas em vez de hardcoded; tokens semânticos consistentes
3. **Patterns**: idioms do projeto respeitados (fail-open, lenient serde, trait-backed IO em core; hook module pattern em rt; sibling-convention em dashboard)
4. **i18n** (só dashboard): labels novas via provider; nenhuma string em PT cravada em código onde deveria ser key
5. **Integration**: contratos entre waves (W2 usa W1, W4 usa W2+W3, W6/W7 usa W4, W8 isolada) funcionam end-to-end
6. **Build**: cargo check + cargo test + pnpm build + tsc --noEmit verdes; sem warnings novos
7. **Elegance**: nomes claros, comentários só onde WHY, sem dead code, sem TODO órfão, sem cargo culting

## Concerns já listados (não precisam ser re-flagged como CRITICAL, mas reviewer deve avaliar se concorda com a justificativa de cada um)

- W1: ALTER spans table (vs tabela nova)
- W1: custos per-agent/per-wave aproximados (fechado em W4 — confirmar)
- W2: BashGuardBlock vs RtkRewrite no site rtk-rewrite (linha 1438-1474)
- W2: wave_id None até W4 wirear env
- W2: 5 cópias do helper Connection (mitigado parcialmente em W3a com open_for)
- W3a: notify ausente do workspace (resolvido em W3b)
- W3a: RtkCommand trait pub
- W3a: open_for constrói-e-descarta SqliteEventStore
- W3b: notify=6 (não 8) pinado por causa do dashboard
- W3b: is_process_alive sem windows-sys (tasklist em Win)
- W3b: /v1/traces é rota nova (legacy metrics/logs intactos)
- W4: tool_use_id via extra map
- W4: filtro de scope movido pra fora da CTE
- W4: 3º fallback silencioso (span sem agent.start)
- W5: .dark class (não prefers-color-scheme)
- W5: dois @theme coexistindo (style.css + theme.css)
- W5: NOTICE.md com placeholders <year>/<authors>
- W6: TokenBreakdown só popula input (W4 não split)
- W6: timeline coexiste (não substituiu)
- W7: EconomyScopeDto separado do core
- W7: 2 commands bonus além do spec
- W8: i18n bindado a useStore.language existente
- W8: top_files_today fix via override no adapter (não core)

## Tarefas

### Core Reviewer Agent
- [ ] Audit packages/core/ files vs 7 categorias
- [ ] Validar que todos os Concerns W1+W3a+W4 acima têm justificativa aceitável
- [ ] Listar CRITICAL/WARNING/NOTE em formato estruturado
- [ ] Listar `## Tactical Fix Candidates` (≤100 LOC, no contract change, no pending decision, no new dep)
- [ ] Verdict final: APPROVED ou REJECTED

### RT Reviewer Agent
- [ ] Audit apps/rt/ files vs 7 categorias
- [ ] Validar Concerns W2+W3b
- [ ] Listar CRITICAL/WARNING/NOTE
- [ ] Listar `## Tactical Fix Candidates`
- [ ] Verdict final

### Dashboard Reviewer Agent
- [ ] Audit apps/dashboard/ files vs 7 categorias (incluindo DS category)
- [ ] Validar Concerns W5+W6+W7+W8
- [ ] Listar CRITICAL/WARNING/NOTE
- [ ] Listar `## Tactical Fix Candidates`
- [ ] Verdict final

### Consolidação
- [ ] Orquestrador (eu) consolida os 3 verdicts em `review/verdict.md`
- [ ] Se algum CRITICAL: fix-loop (Step 19b do SKILL)
- [ ] Se todos APPROVED: emit review.result event, avançar para QA (Wave 10)
