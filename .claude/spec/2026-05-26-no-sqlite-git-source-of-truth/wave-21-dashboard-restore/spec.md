# Restore dashboard telemetry behavior — replace W6B stubs with real NDJSON readers

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: light
### Checkpoint: 2026-05-27T15:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec corretiva de [[2026-05-26-no-sqlite-git-source-of-truth]]. Wave 6B (wave-20-dashboard,
commit 723ad1a) migrou `apps/dashboard/src-tauri/src/telemetry.rs` de 1880 → 570 LOC,
trocando SQLite por NDJSON. Mas ~7 funções públicas ficaram como **stub vazio**
(`Default::default()`, `Vec::new()`) sob justificativa "fail-open preservando assinatura
Tauri". Isso é **regressão funcional**: widgets do dashboard (RTK summary, routing
breakdown, hook counts, agent activity, workflow phases, tool breakdown, measured
costs) passaram a mostrar "—"/zero.

Esta sub-spec restaura comportamento implementando reader real (NDJSON ou
filesystem) para cada função stubada, lendo os mesmos eventos que `apps/rt/` já
está escrevendo desde W5/W2/W3.

### Inventário

| # | Função | Fonte real | Status |
|---|--------|-----------|--------|
| 1 | `rtk_summary(repo)` | subprocess `rtk gain -f json --daily -p` (cwd=repo) | restaurar |
| 2 | `rtk_summary_global()` | subprocess `rtk gain -f json --daily` | restaurar |
| 3 | `hook_fire_counts(repo, since)` | filesystem `.claude/.metrics/*.jsonl` (legacy channel ainda escrito) | restaurar |
| 4 | `routing_breakdown(repo, since)` | filesystem `.claude/.metrics/model-routing-gate.jsonl` | restaurar |
| 5 | `workflow_by_phase(repo)` | NDJSON `event=="pipeline.phase"` cross-spec | restaurar |
| 6 | `tool_breakdown(repo)` | NDJSON `event=="tool.use"` cross-spec, agrega `payload.tool` | restaurar |
| 7 | `agent_activity(repo)` | NDJSON `event=="agent.start"`/`"agent.stop"` cross-spec | restaurar |
| 8 | `measured(repo)` | NDJSON `event=="pipeline.telemetry.run"` cross-spec, soma `payload.input_tokens`+`payload.output_tokens` | restaurar |
| 9 | `dashboard_spec_trace(project, spec)` | fix signature + minimal tree (raiz spec + tool list, sem agrupamento agent completo) | restaurar (parcial) |

**Não inclui (gap arquitetural — reportar)**: `dashboard_economy_summary`,
`dashboard_economy_savings_breakdown`, `dashboard_economy_context_routing`,
`dashboard_economy_per_spec_costs`, `dashboard_economy_per_wave_costs`,
`dashboard_prompt_economy`. Razões:
  - Assinatura W6B (`_repo_path: String`) NÃO corresponde ao que o frontend chama
    (`scope: EconomyScopeDto`) — frontend trava ao chamar.
  - Implementação real depende de readers em `mustard_core::economy` que ainda
    consultam SQLite (`store::open_for` retorna `Connection` SQLite). Migrar essa
    camada para NDJSON é trabalho próprio (W7+), fora do escopo desta correção
    tática.
  - **Ação nesta sub-spec**: APENAS corrigir assinatura para `scope:
    EconomyScopeDto` (evita panic) + manter stub Default. Tag explícito no doc-comment
    como "behavioral gap — pendente W7+".

## Critérios de Aceitação

- [ ] AC-21-1: `cargo build -p mustard-dashboard` passa. Command: `cargo build -p mustard-dashboard`
- [ ] AC-21-2: `cargo test -p mustard-dashboard --no-run` compila com 0 erros. Command: `cargo test -p mustard-dashboard --no-run`
- [ ] AC-21-3: `rtk_summary(repo)` invoca `rtk gain -f json --daily -p` (cwd=repo) e retorna `RtkBlock` com `available=true` quando RTK responde. Command: `node -e "const fs=require('fs'); const src=fs.readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/rtk_summary\\s*\\([^)]*\\)\\s*->\\s*RtkBlock\\s*\\{[\\s\\S]*?Command::new[\\s\\S]*?\\\"rtk\\\"/.test(src)){process.exit(1)}"`
- [ ] AC-21-4: `hook_fire_counts` lê `.claude/.metrics/*.jsonl` e retorna `Vec<HookFireCount>` agregado por hook. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/hook_fire_counts[\\s\\S]*?\\.metrics[\\s\\S]*?\\.jsonl/.test(s))process.exit(1)"`
- [ ] AC-21-5: `routing_breakdown` lê `.claude/.metrics/model-routing-gate.jsonl` e agrupa por subagent_type. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/routing_breakdown[\\s\\S]*?model-routing-gate\\.jsonl/.test(s))process.exit(1)"`
- [ ] AC-21-6: `workflow_by_phase` lê NDJSON `pipeline.phase` events cross-spec. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/workflow_by_phase[\\s\\S]*?pipeline\\.phase/.test(s))process.exit(1)"`
- [ ] AC-21-7: `tool_breakdown` lê NDJSON `tool.use` events cross-spec. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/tool_breakdown[\\s\\S]*?tool\\.use/.test(s))process.exit(1)"`
- [ ] AC-21-8: `agent_activity` lê NDJSON `agent.start`/`agent.stop` events cross-spec. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/agent_activity[\\s\\S]*?agent\\.start[\\s\\S]*?agent\\.stop/.test(s))process.exit(1)"`
- [ ] AC-21-9: `measured` lê NDJSON `pipeline.telemetry.run` events e soma tokens. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/pub fn measured[\\s\\S]*?pipeline\\.telemetry\\.run/.test(s))process.exit(1)"`
- [ ] AC-21-10: `dashboard_spec_trace` assinatura aceita `(project_path, spec_name)` em vez de `(session_id, tool_use_id, started_at_ms)`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/pub fn dashboard_spec_trace[\\s\\S]*?project_path:\\s*String[\\s\\S]*?spec_name:\\s*String/.test(s))process.exit(1)"`
- [ ] AC-21-11: `dashboard_economy_*` (5 cmds) + `dashboard_prompt_economy` aceitam `EconomyScopeDto` (não `String repo_path`), evitando panic na chamada frontend. O nome do parâmetro pode ser `scope` ou `_scope` (Rust convention para param não-lido). Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fns=['dashboard_economy_summary','dashboard_economy_savings_breakdown','dashboard_economy_context_routing','dashboard_economy_per_spec_costs','dashboard_economy_per_wave_costs','dashboard_prompt_economy']; for(const f of fns){const re=new RegExp('pub fn '+f+'\\\\(_?scope:\\\\s*EconomyScopeDto');if(!re.test(s)){console.error('miss',f);process.exit(1)}}"`

## Plano

## Arquivos

- `apps/dashboard/src-tauri/src/telemetry.rs` (único arquivo modificado)

## Tarefas

1. Adicionar imports: `std::process::Command`, `std::collections::HashMap`, `crate::process_util::no_window_command`.
2. Implementar `rtk_summary` + `rtk_summary_global` via subprocess `rtk gain` (idêntico ao pre-W6B, sem rusqlite).
3. Implementar `hook_fire_counts` lendo `.claude/.metrics/*.jsonl` (skip `rtk-gain`, `rtk-rewrite`, `budget-observations`).
4. Implementar `routing_breakdown` lendo `.claude/.metrics/model-routing-gate.jsonl`, group by subagent_type/pipeline_type.
5. Implementar `workflow_by_phase`, `tool_breakdown`, `agent_activity`, `measured` via `EventReader` cross-spec — filtrando por `event.raw["event"]` (não `event.kind`, que é a classificação).
6. Fix `dashboard_spec_trace` signature + minimal tree: spec node + flat tool node list per spec (parser via NDJSON `tool.use`). Sem build full 4-level tree — tradeoff explícito no doc-comment, restauração progressiva.
7. Fix signatures dos 6 `dashboard_economy_*` + `dashboard_prompt_economy` para aceitar `scope: EconomyScopeDto` (preserva default, doc-comment marca behavioral gap).
8. Adicionar `EconomyScopeDto` (copy from pre-W6B telemetry).
9. Verify: `rtk cargo build -p mustard-dashboard` + `rtk cargo test -p mustard-dashboard --no-run`.

## Dependências

Depende de W2 (NDJSON event writer escrevendo `tool.use`, `agent.start/stop`, `pipeline.phase`, `pipeline.telemetry.run`); W6B (estrutura atual do telemetry.rs); legacy `.claude/.metrics/*.jsonl` continua escrito por `mustard_core::metrics::emit_metric`. Consome `mustard_core::events::EventReader`, `mustard_core::ClaudePaths`.

## Limites

- Single-file (telemetry.rs) — sub-spec ≤1 arquivo no diff principal.
- `dashboard_economy_*` (6 cmds): apenas fix de signature (preserva Default) + documentação do gap. Implementação real depende de migrar `mustard_core::economy::reader` para NDJSON, fora do escopo desta correção.
- `dashboard_spec_trace`: tree minimal (spec + tool list), não 4-level (spec → wave → agent → tool). Restauração full fica para correção subsequente.
- Sem novos tests (W6B já tem cobertura de fixture-driven shape).
- Commit message: `fix(wave-21/dashboard): restore telemetry behavior — replace W6B stubs with real NDJSON readers`
