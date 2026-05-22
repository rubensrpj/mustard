# Finalização da camada de domínio SDD (qa.result, legado e telemetria visual)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-20T20:30:00Z
### Lang: pt

## PRD

## Contexto

As waves 1 a 5 da auditoria de 2026-05-20 entregaram a fundação da camada de domínio SDD do Mustard: o crate `mustard-specsdb` virou a fonte única de leitura, o materializer `mustard-rt run rebuild-specs` mantém `specs`/`metrics_projection` vivas, os emissores de `HarnessEvent` foram corrigidos para preencher `spec`, e o dashboard delega aos adapters `*_v2`. Restam três frentes que ficaram fora do escopo daquelas waves e que, sem fechá-las, deixam buracos concretos:

1. **`qa.result` não é emitido em todo CLOSE.** O subcomando `qa_run::run` já constrói o evento com `spec` populado (linha 269 de `apps/rt/src/run/qa_run.rs`), mas só é executado quando o usuário invoca `/mustard:qa` manualmente. As 73 specs do banco real mostram `ac_passed=0/ac_total=0` justamente porque nunca rodaram QA pós-feature — o reader `mustard-specsdb` folda corretamente, só não há evento pra foldar.
2. **Legado de `spec_views::spec_*` ainda no binário.** Os adapters `*_v2` são chamados pelos Tauri commands desde a Wave 4, mas as funções `spec_card`, `spec_waves`, `spec_quality`, `spec_timeline`, `workspace_summary` antigas (≈ 700 linhas) ficaram como fallback defensivo. Mais 200 linhas de testes em `spec_views_test.rs` validam o caminho SQL hardcoded que ninguém mais executa. Compilam, geram avisos sobre código não usado e mascaram a fonte real do projeto.
3. **A "Sala de Operações multi-track" perdeu protagonismo visual.** A spec `2026-05-19-telemetry-dashboard-redesign` entregou `PipelineTimeline`, `EffortHeatmap`, `HistoryStrip`, `CriteriaPanel` como componentes ricos; a spec `2026-05-20-sdd-dashboard-restructure` apagou `Telemetry.tsx` mas não migrou esses componentes para `Workspace.tsx`. Hoje a Visão Geral mostra `WorkspaceStatusBar` + `SpecTracksList` + `WorkspaceAlertsColumn` — funcional, mas magro perto do que existia. Os componentes ainda estão em `src/components/telemetry/`, sem consumidor.

A solicitação veio do usuário em 2026-05-20 logo após a entrega das 5 waves anteriores: "crie uma spec para os próximos passos". Esta spec é a entrega final do diagnóstico — depois dela, o trabalho de domínio SDD está fechado e o dashboard volta a refletir o que a ferramenta promete.

## Usuários/Stakeholders

Mantenedores do Mustard que consomem o dashboard como interface SDD primária — em particular Rubens, que rastreou o drift via screenshots reais em 2026-05-20. Indiretamente, qualquer usuário do `mustard-dashboard` que abre uma spec encerrada e espera ver `3/3 ACs passing` em vez de `0/0`, ou que abre a Visão Geral e espera ver o protagonismo visual de uma ferramenta de pipeline.

## Métrica de sucesso

- `mustard-rt run rebuild-specs` reporta `ac_total > 0` para cada spec em `completed/` que tem seção `## Critérios de Aceitação` no `spec.md`.
- Compilação do workspace **não emite nenhum `dead_code` warning** sobre `spec_views::spec_card` / `spec_waves` / `spec_quality` / `spec_timeline` / `workspace_summary` legacy.
- A página `Workspace.tsx` renderiza `PipelineTimeline` como elemento hero (full-width) e `EffortHeatmap` abaixo, sem regredir o `SpecTracksList` introduzido pela spec restructure.
- Auditoria Hallmark em `Workspace.tsx` retorna 0 critical findings após a restauração visual.
- Toda a UI mantém a paleta mustard yellow (`--primary: #dfab01`/`#e6c84a`) — nenhum componente restaurado reintroduz tokens violet/indigo.

## Não-Objetivos

- **Não reescrever** `mustard-specsdb`, `rebuild_specs` ou os adapters `*_v2`. As waves 1-5 fecharam aquela superfície; esta spec consome o que existe.
- **Não criar uma página `Telemetry.tsx` separada.** A spec `sdd-dashboard-restructure` decidiu deliberadamente consolidar telemetria em Visão Geral + Specs; reverter isso quebra a navegação. A restauração visual de Wave 3 enriquece a Visão Geral existente, não ressuscita a rota antiga.
- **Não tocar o OTEL collector** (`apps/rt/src/run/otel/collector.rs`). Captura de telemetria nativa está fora do escopo.
- **Não introduzir migração com banner ou flag.** O Mustard está em desenvolvimento (memory `feedback_no_migration_dev_phase`); o legado de `spec_views` é deletado direto.
- **Não fazer QA visual automatizada via Playwright/Selenium.** AC visuais ficam como item manual na Checklist — o ambiente Tauri desktop não dá render headless confiável e o user pediu explicitamente para ele mesmo testar.
- **Não emitir `qa.result` para specs históricas via heurística (parsing de `[x]` no `spec.md`).** Heurística de checkbox não é resultado de execução; preferimos `ac_total > 0, ac_passed = 0` honesto a um `ac_passed = ac_total` fabricado.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou. Padrão `node -e "...includes()"` (cross-shell-safe per memória `feedback_ac_cross_shell_windows.md`).

- [x] AC-1: Workspace inteiro compila limpo — Command: `cargo build --workspace`
- [x] AC-2: Workspace inteiro passa testes — Command: `cargo test --workspace --exclude mustard-dashboard`
- [x] AC-3: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: Dashboard backend testes passam — Command: `cargo test -p mustard-dashboard`
- [x] AC-5: Subcomando `qa-run-all` registrado em `apps/rt/src/run/mod.rs` (variant + dispatch + module) — Command: `node -e "const c=require('fs').readFileSync('apps/rt/src/run/mod.rs','utf8');for(const s of ['mod qa_run_all','QaRunAll','qa_run_all::run']){if(!c.includes(s))process.exit(1)}"`
- [x] AC-6: `complete_spec.rs` invoca `qa_run::run` antes do mark_followup — Command: `node -e "const c=require('fs').readFileSync('apps/rt/src/run/complete_spec.rs','utf8');process.exit(c.includes('qa_run::run')?0:1)"`
- [x] AC-7: Legacy `spec_card`/`spec_waves`/`spec_quality`/`spec_timeline`/`workspace_summary` removidos do `spec_views.rs` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');const legacy=['pub fn spec_card(','pub fn spec_waves(','pub fn spec_quality(','pub fn spec_timeline(','pub fn workspace_summary('];process.exit(legacy.some(s=>c.includes(s))?1:0)"`
- [x] AC-8: Helpers preservados (`spec_events`, `spec_action`) e adapters `*_v2` permanecem — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');for(const s of ['pub fn spec_events(','pub fn spec_action(','pub fn spec_card_v2(','pub fn spec_waves_v2(','pub fn workspace_summary_v2(']){if(!c.includes(s))process.exit(1)}"`
- [x] AC-9: `Workspace.tsx` importa `PipelineTimeline` e `EffortHeatmap` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');for(const s of ['PipelineTimeline','EffortHeatmap']){if(!c.includes(s))process.exit(1)}"`
- [x] AC-10: `SpecTracksList`, `WorkspaceAlertsColumn`, `WorkspaceStatusBar` preservados em `Workspace.tsx` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Workspace.tsx','utf8');for(const s of ['SpecTracksList','WorkspaceAlertsColumn','WorkspaceStatusBar']){if(!c.includes(s))process.exit(1)}"`
- [x] AC-11: Zero referência a `indigo`/`violet`/`sky`/`emerald`/`amber`/`rose` em arquivos novos/modificados — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/pages/Workspace.tsx'];for(const f of files){const c=fs.readFileSync(f,'utf8');for(const color of ['indigo-','violet-','sky-','emerald-','amber-','rose-']){if(c.includes(color))process.exit(1)}}"`
- [x] AC-12: Auditoria Hallmark em `Workspace.tsx` registra 0 critical findings em `.claude/.harness/audit-workspace-restored.md` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('.claude/.harness/audit-workspace-restored.md'))process.exit(1);const c=fs.readFileSync('.claude/.harness/audit-workspace-restored.md','utf8');if(/critical.*[1-9]/i.test(c))process.exit(2)"`

## Plano

## Informações da Entidade

Sem entidade nova. Esta spec consome:

- `mustard_specsdb::{SpecReader, SqliteSpecReader}` — domínio definido na Wave 2 de 2026-05-20.
- `mustard_rt::run::qa_run` — emissor de `qa.result` já existente, com `current_spec()` corrigido na Wave 1.
- `mustard_rt::run::rebuild_specs::rebuild_one` — materializer single-spec entregue na Wave 3.
- Componentes `apps/dashboard/src/components/telemetry/{PipelineTimeline, EffortHeatmap, PhaseStation, …}.tsx` — entregues pela spec `2026-05-19-telemetry-dashboard-redesign`, hoje órfãos.

Único shape novo (interno ao `qa-run-all`):

| Shape | Campos | Origem |
|---|---|---|
| `QaBatchReport` | `{ ran: u32, failed: u32, skipped: u32, errors: Vec<String> }` | retorno JSON do `qa-run-all` |

## Arquivos

```
apps/rt/src/run/qa_run.rs                                   — extrai run() em run_for_spec() reutilizável
apps/rt/src/run/qa_run_all.rs                               — novo módulo: itera specs ativas e roda qa_run
apps/rt/src/run/mod.rs                                      — registra RunCmd::QaRunAll
apps/rt/src/run/complete_spec.rs                            — invoca qa_run::run_for_spec antes do mark_followup
apps/rt/tests/complete_spec_emits_qa.rs                     — novo: teste integração que confirma qa.result após CLOSE

apps/dashboard/src-tauri/src/spec_views.rs                  — deleta 5 funções legacy + shapes não-usados
apps/dashboard/src-tauri/tests/spec_views_test.rs           — deleta os 7 testes que validavam o caminho SQL antigo
apps/dashboard/src-tauri/src/lib.rs                         — remove imports `db::with_db` ainda atrelados ao legado

apps/dashboard/src/pages/Workspace.tsx                      — incorpora PipelineTimeline (hero) + EffortHeatmap
apps/dashboard/src/components/workspace/WorkspaceLiveHero.tsx — novo: wrapper que combina pulse + PipelineTimeline
apps/dashboard/src/hooks/useTelemetryHeatmap.ts             — confirmar refetchInterval (já existente)

.claude/.harness/audit-workspace-restored.md                — output do Hallmark audit (gerado Wave 3)
```

## Tarefas

### Wave 1 — rt: qa.result automático em CLOSE

- [x] Extrair lógica core de `qa_run::run` em `pub fn run_for_spec(spec: &str, format: &str) -> QaRunOutcome` que retorna o veredito sem `process::exit`.
- [x] Criar `apps/rt/src/run/qa_run_all.rs`: itera `SqliteSpecReader::list_specs(SpecFilter::Active)`, chama `run_for_spec` para cada uma, agrega `QaBatchReport`. Saída JSON.
- [x] Registrar `RunCmd::QaRunAll` em `apps/rt/src/run/mod.rs` + dispatch.
- [x] Em `complete_spec::run` (caminho `mark_followup` e `archive`), invocar `qa_run::run_for_spec(spec, "json")` ANTES de `rebuild_one_fail_open`. Fail-open — log e segue se QA falhar.
- [x] Teste integração `apps/rt/tests/complete_spec_emits_qa.rs`: seed spec com `## Critérios de Aceitação` mínimo; chamar `complete_spec::run`; verificar via `SqliteEventStore::query(spec)` que existe ≥1 evento `qa.result`.
- [x] `cargo build -p mustard-rt && cargo test -p mustard-rt`.

### Wave 2 — dashboard: retirar legado `spec_views::spec_*`

- [x] Em `apps/dashboard/src-tauri/src/spec_views.rs`: deletar funções `spec_card` (linha 157), `spec_waves` (340), `spec_quality` (474), `spec_timeline` (551), `workspace_summary` (921). Manter `spec_events`, `spec_action`, helpers de track/segment e adapters `*_v2`.
- [x] Em `apps/dashboard/src-tauri/tests/spec_views_test.rs`: deletar testes que invocam as funções removidas. Manter os que cobrem `spec_events`/`spec_action` ou os adapters.
- [x] Em `apps/dashboard/src-tauri/src/lib.rs`: remover qualquer `db::with_db` órfão que apontava para o caminho antigo (a Wave 4 já moveu os Tauri commands; confirmar que ninguém ficou pendurado).
- [x] Auditar restantes shapes do `spec_views.rs` — deletar qualquer struct (`PhaseSegment`, `SpecTrack`, etc.) que só era consumida pelas funções removidas.
- [x] `cargo build -p mustard-dashboard && cargo test -p mustard-dashboard`.

### Wave 3 — dashboard: restauração visual da Visão Geral

- [x] Confirmar que `apps/dashboard/src/components/telemetry/{PipelineTimeline, EffortHeatmap, PhaseStation, EffortHeatmap}.tsx` existem.
- [x] Criar `apps/dashboard/src/components/workspace/WorkspaceLiveHero.tsx`: combina `WorkspaceStatusBar` (existente) + `PipelineTimeline` agora como hero (full-width), animação `wave-glow` na fase ativa. Recebe `summary: WorkspaceSummary` como prop.
- [x] Em `Workspace.tsx`: substituir `<WorkspaceStatusBar summary={summary} />` standalone por `<WorkspaceLiveHero summary={summary} />`. Logo abaixo, antes da grid (main + aside): inserir `<EffortHeatmap />` (consumindo `useTelemetryHeatmap`) em largura total.
- [x] Garantir paleta: nenhum classname novo com `indigo`/`violet`/`sky`/`emerald`/`amber`/`rose`. Usar `--color-accent-mustard` para acentos e `--color-ok` / `--color-error` para semantics.
- [x] Tabular-nums em todo número exibido (heatmap counts, pulse rate).
- [x] Rodar `hallmark` skill no `Workspace.tsx` restaurado → output em `.claude/.harness/audit-workspace-restored.md`. Esperado: 0 critical.
- [x] `pnpm --filter mustard-dashboard build && pnpm --filter mustard-dashboard test`.

### Wave 4 — Visual QA (manual, AC documental)

- [ ] Rodar `pnpm tauri:dev` localmente.
- [ ] Navegar: Visão Geral → conferir hero (PipelineTimeline) + heatmap + SpecTracks lateral + Alerts.
- [ ] Specs → conferir lista com badges `ativa`/`concluída`/`—` (no-events); clicar uma spec e ver drill-down com Ondas/Qualidade/Timeline/Eventos.
- [ ] Economia → conferir que tokens economizados mostra "—" quando RTK indisponível.
- [ ] Knowledge → conferir dedup (sem repetições).
- [ ] Capturar screenshots e anotar regressões em `.claude/.harness/wave-4-visual-qa.md`.

## Dependências

- Waves 1-5 da auditoria 2026-05-20 (todas closed antes desta spec): fornecem `mustard-specsdb`, `rebuild-specs`, attribution corrigida, adapters `*_v2` e UI honesta. Esta spec **assume** todo esse trabalho landed.
- Wave 1 (rt qa.result automático) **bloqueia** Wave 2/3 quanto à validação real — sem `qa.result` em CLOSE, os AC ainda mostram `0/0` no dashboard mesmo com a UI honesta.
- Wave 2 (legado) e Wave 3 (visual) podem rodar em paralelo entre si — tocam camadas independentes.
- Wave 4 (visual QA manual) depende de todas as anteriores landed.

## Limites

- `apps/rt/src/run/{qa_run.rs, qa_run_all.rs, complete_spec.rs, mod.rs}`
- `apps/rt/tests/complete_spec_emits_qa.rs` (novo)
- `apps/dashboard/src-tauri/src/{spec_views.rs, lib.rs}`
- `apps/dashboard/src-tauri/tests/spec_views_test.rs`
- `apps/dashboard/src/pages/Workspace.tsx`
- `apps/dashboard/src/components/workspace/WorkspaceLiveHero.tsx` (novo)
- `.claude/.harness/audit-workspace-restored.md` (gerado)

**Fora dos limites:**

- `mustard-specsdb` (fechada na Wave 2 da auditoria — domínio congelado)
- `mustard-core` (não há schema novo)
- OTEL collector + qualquer ingestão de telemetria nativa
- Pages `Specs.tsx`, `Economia.tsx`, `Knowledge.tsx` (já entregues)
- Routing / Sidebar / Topbar (já entregues pela spec restructure)
- Identidade visual / paleta de cores (já entregue Wave 1 da auditoria)
- Heurística de parsing de `[x]` no spec.md para backfill de ACs antigos — explicitamente recusado em Não-Objetivos

## Checklist

- [x] Wave 1 — qa.result automático em CLOSE
- [x] Wave 2 — Retirar `spec_views` legacy
- [x] Wave 3 — Restauração visual da Visão Geral
- [ ] Wave 4 — Visual QA manual (documentado)
- [ ] `cargo build --workspace` verde
- [ ] `cargo test --workspace --exclude mustard-dashboard` verde
- [x] `pnpm --filter mustard-dashboard build` verde
- [ ] AC-1 a AC-12 todos com `[x]`
