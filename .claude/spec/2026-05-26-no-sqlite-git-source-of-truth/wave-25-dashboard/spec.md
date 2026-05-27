# Wire dashboard economy commands to real NDJSON readers (W7D — 6 commands + spec_trace 4-level)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: dashboard
### Checkpoint: 2026-05-27T20:00:00Z
### Lang: pt-BR
### Parent: 2026-05-26-no-sqlite-git-source-of-truth

## PRD

## Contexto

Sub-spec W7D da [[2026-05-26-no-sqlite-git-source-of-truth]]. Após W7A/W7B/W7C
migrarem `mustard_core::economy` pra NDJSON, é hora de remover o **dívida de wave-21**:
6 commands Tauri no dashboard que ficaram com body `Default::default()` porque
dependiam dos readers SQLite. Esta sub-spec implementa o body real chamando
`mustard_core::economy::reader::*` agora migrados.

### Estado atual (entrada — após W7A/W7B/W7C)

`apps/dashboard/src-tauri/src/telemetry.rs` tem 7 commands com gap:

| Command | Estado atual | Estado alvo |
|---|---|---|
| `dashboard_prompt_economy` | retorna `{cost: {usd_total:0,...}}` default | aggregate NDJSON cross-spec (cost via `pipeline.telemetry.metric` + subtractions via `pipeline.economy.savings.*` + claude_events via `pipeline.telemetry.metric:session.count`) — segue shape do TS `PromptEconomy` |
| `dashboard_economy_summary` | `EconomySummary` zerada | chama `mustard_core::economy::reader::economy_summary(&PathBuf::from(scope.project_path()), scope)` |
| `dashboard_economy_savings_breakdown` | `{by_source: []}` | chama `savings_breakdown(&project, scope)` |
| `dashboard_economy_context_routing` | ratios zero | chama `context_routing_quality(&project, scope)` |
| `dashboard_economy_per_spec_costs` | `[]` | chama `per_spec_costs(&project, scope)` |
| `dashboard_economy_per_wave_costs` | `[]` | chama `per_wave_costs(&project, scope)` |
| `dashboard_spec_trace` | minimal: spec root + flat tool list | 4-level tree (spec → wave → agent → tool com roll-up de tokens via `per_agent_costs` e correlation por `tool_use_id`) |

### Decisões de design

1. **Scope DTO conversion**: `EconomyScopeDto` (Tauri-side) converte para `EconomyScope` core
   via método `to_core() -> EconomyScope` (helper local em telemetry.rs).
2. **Project-path resolution**: cada scope variant carrega seu próprio project path. Para
   `AllProjects`, itera projetos chamando reader por projeto + merge (mantém parity com
   `MultiProjectReader` que agora também é fs-based).
3. **`dashboard_prompt_economy`**: shape de saída ainda é o `PromptEconomy` original (não
   `EconomySummary`). Implementação re-aggrega NDJSON; usa `economy_summary` no bloco de
   cost (mas só os campos usd_total + by_model/by_session). Subtractions vem dos savings.
4. **`dashboard_spec_trace` (4-level)**: usa `per_agent_costs` para roll-up de tokens; correlaciona
   `tool.use` (NDJSON `event=="tool.use"`) com `agent.start/stop` por `tool_use_id` no payload.
   Estrutura: `spec → wave_id (do payload.wave_id) → agent_id → tool list`.
5. **Anti-stub**: AC-W7D-FINAL verifica grep que NENHUM dos 6 commands tem `Default::default()`
   ou `Vec::new()` no body principal (só em fallback de fail-open explicit).

### Hard rule — sem stub

`AC-W7D-NO-STUB`: `git grep -nE "Default::default\(\)|return None|Vec::new\(\)" apps/dashboard/src-tauri/src/telemetry.rs` listado, e por inspeção manual, **nenhum dos 6 commands de economia** + `dashboard_spec_trace` mostra Default no body principal.

## Critérios de Aceitação

- [x] AC-W7D-1: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` verde. Command: `cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-W7D-2: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --no-run` compila 0 erros. Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --no-run`
- [x] AC-W7D-3: `dashboard_economy_summary` body chama `mustard_core::economy::reader::economy_summary`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_economy_summary[\\s\\S]*?\\n\\}/); if(!fn||!/economy::reader::economy_summary|economy_summary\\(/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-4: `dashboard_economy_savings_breakdown` body chama `savings_breakdown`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_economy_savings_breakdown[\\s\\S]*?\\n\\}/); if(!fn||!/savings_breakdown\\(/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-5: `dashboard_economy_context_routing` body chama `context_routing_quality`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_economy_context_routing[\\s\\S]*?\\n\\}/); if(!fn||!/context_routing_quality\\(/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-6: `dashboard_economy_per_spec_costs` body chama `per_spec_costs`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_economy_per_spec_costs[\\s\\S]*?\\n\\}/); if(!fn||!/per_spec_costs\\(/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-7: `dashboard_economy_per_wave_costs` body chama `per_wave_costs`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_economy_per_wave_costs[\\s\\S]*?\\n\\}/); if(!fn||!/per_wave_costs\\(/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-8: `dashboard_prompt_economy` body agrega NDJSON real (não default). Command: `node -e "const fs=require('fs'); const src=fs.readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); if(!/dashboard_prompt_economy[\s\S]*?(EventReader|economy::reader|walk_ndjson_events)/.test(src)){process.exit(1)}"`
- [x] AC-W7D-9: `dashboard_spec_trace` tree é 4-level (spec → wave → agent → tool). Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const fn=s.match(/pub fn dashboard_spec_trace[\\s\\S]*?\\n\\}/); if(!fn||!/wave|agent_id/.test(fn[0])){process.exit(1)}"`
- [x] AC-W7D-10 (ANTI-STUB FINAL): inspeção dos 6 commands de economia confirma que body principal NÃO é `Default::default()`/`Vec::new()`. Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8'); const cmds=['dashboard_prompt_economy','dashboard_economy_summary','dashboard_economy_savings_breakdown','dashboard_economy_context_routing','dashboard_economy_per_spec_costs','dashboard_economy_per_wave_costs']; for(const c of cmds){const fn=s.match(new RegExp('pub fn '+c+'\\\\([^)]*\\\\)[^{]*\\\\{([\\\\s\\\\S]*?)\\\\n\\\\}')); if(!fn){console.error('miss',c);process.exit(1)} const body=fn[1].trim(); if(/^serde_json::json!\\(/.test(body)||/^Default::default\\(\\)$/.test(body)||body.split('\\n').length<3){console.error('stub-like',c);process.exit(1)}}"`

## Plano

## Arquivos

- `apps/dashboard/src-tauri/src/telemetry.rs` — UPDATE (rewrite dos 6 commands + spec_trace 4-level)

## Tarefas

1. **Helper `EconomyScopeDto::to_core`**:
   ```rust
   impl EconomyScopeDto {
       fn to_core(&self) -> (PathBuf, mustard_core::economy::EconomyScope) {
           // map kind → EconomyScope, returning project root + scope
       }
   }
   ```
2. **`dashboard_economy_summary`**: chama `economy_summary(&project, scope)`; serializa o struct (já tem `Serialize`).
3. **`dashboard_economy_savings_breakdown`**: chama `savings_breakdown(&project, scope)`; serializa.
4. **`dashboard_economy_context_routing`**: chama `context_routing_quality(&project, scope)`; serializa.
5. **`dashboard_economy_per_spec_costs`**: chama `per_spec_costs(&project, scope)`; serializa Vec.
6. **`dashboard_economy_per_wave_costs`**: chama `per_wave_costs(&project, scope)`; serializa Vec.
7. **`dashboard_prompt_economy`**: agrega 3 blocos:
   - `cost`: walk NDJSON `pipeline.telemetry.metric` events (filter metric=`claude_code.cost.usage`), sum + group by model/session.
   - `subtractions`: walk `pipeline.economy.savings.*` events, soma `tokens_saved` (proxy de bytes via `tokens × 4`), group by wave.
   - `claude_events`: count distinct `session_id` em `pipeline.telemetry.metric`, sum `active_time` se evento `claude_code.active_time` presente (else 0).
   - `freshness`: max ts dos eventos relevantes + check de health do collector via canary file.
8. **`dashboard_spec_trace` 4-level**:
   - Lê NDJSON do spec dir: `<project>/.claude/spec/{spec_name}/.events/*.ndjson`.
   - Agrupa: `tool.use` events tem `payload.wave_id` (ou env-derived) + `payload.tool_use_id`; `agent.start/stop` tem `payload.agent_id` e correlaciona via tool_use_id.
   - Roll-up tokens: chama `per_agent_costs(&project, EconomyScope::Spec{project,spec})` pra somar tokens por agent_id no spec.
   - Tree: spec root → wave nodes (group by payload.wave_id) → agent nodes (group by agent_id) → tool nodes (cada tool.use).
9. **Tests**: adiciona ao módulo `#[cfg(test)] mod tests` em telemetry.rs:
   - `economy_summary_via_dto_uses_core_reader` — fixture: tmpdir + 1 NDJSON event `pipeline.telemetry.metric:cost.usage`, scope Project, assert `total_cost_usd_micros > 0`.
   - `savings_breakdown_via_dto_aggregates_ndjson` — fixture: 2 savings events, assert `per_source` populado.
   - `spec_trace_returns_4_level_tree` — fixture: 1 spec, 1 wave, 1 agent, 2 tool events, assert tree depth = 4.
10. **Verify**: `rtk cargo build --manifest-path apps/dashboard/src-tauri/Cargo.toml` + `rtk cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml --no-run` + AC-W7D-10 anti-stub.

## Dependências

- Requer W7A + W7B + W7C já commitados.

## Limites

- 1 arquivo (telemetry.rs), update grande. Justificativa: o arquivo já é o homehome único dos 7 commands; quebrar em mais files quebra a coesão SRP da unidade Tauri telemetry surface.
- Modelo: opus.
- Commit message: `feat(wave-7/dashboard): W7D — wire 6 economy commands + spec_trace 4-level to NDJSON readers`

<!-- wikilinks-footer-start -->
- [2026-05-26-no-sqlite-git-source-of-truth](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->