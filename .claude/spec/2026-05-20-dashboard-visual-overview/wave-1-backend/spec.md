# Wave 1a — Backend Tauri commands para agregações da Visão Geral

### Parent: [[2026-05-20-dashboard-visual-overview]]
### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full (wave)
### Checkpoint: 2026-05-20T23:59:00Z
### Lang: pt

## PRD

## Contexto

A Visão Geral redesenhada precisa de 3 agregações que hoje não existem em comando Tauri: total de tokens economizados (com top pipelines), atividade do mês (uma linha por dia para um mês/ano dado, com contagem de eventos e fase predominante) e feed de eventos cronológico reverso. As views existentes (`dashboard_*`, `specs_from_db`, `workspace_summary`) retornam dados de "agora" — nenhuma cobre o eixo temporal navegável (mês inteiro arbitrário) nem feed contínuo. Sem esses comandos, os componentes UI da Wave 3 ficam dependendo de derivações no cliente que duplicam SQL no JavaScript.

## Métrica de sucesso

Três `#[tauri::command]` registrados em `main.rs`, callable do front via `invoke()`, devolvendo os shapes documentados abaixo. `cargo check` passa.

## Não-Objetivos

- Não criar novo módulo Rust — extender `spec_views.rs` existente.
- Não cachear resultados (server-side) — leitura direta do `mustard.db`.
- Não filtrar por usuário/projeto além do `project_path` recebido.
- Não modificar `mustard-core` nem o schema do `mustard.db`.

## Acceptance Criteria

- [ ] AC-1: Cargo check passa — Command: `cargo check -p mustard-dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [ ] AC-2: 3 commands declarados — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');['dashboard_token_summary','dashboard_month_activity','dashboard_events_feed'].forEach(c=>{if(!t.includes('fn '+c))throw new Error('missing fn '+c)})"`
- [ ] AC-3: 3 commands registrados no handler — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/main.rs','utf8');['dashboard_token_summary','dashboard_month_activity','dashboard_events_feed'].forEach(c=>{if(!t.includes(c))throw new Error('missing reg '+c)})"`

## Plano

## Arquivos (~2)

```
apps/dashboard/src-tauri/src/spec_views.rs   (modify — +3 fns + structs)
apps/dashboard/src-tauri/src/main.rs         (modify — +3 handlers)
```

## Tarefas

### Backend Agent

- [x] Declarar structs serializáveis em `spec_views.rs`:
  - `TokenSummary { total_saved: i64, top_pipelines: Vec<TopPipeline> }`, `TopPipeline { spec: String, saved: i64 }`
  - `DayActivity { date: String /* YYYY-MM-DD */, event_count: i32, top_phase: Option<String> }`
  - `FeedEvent { id: String, ts: String /* ISO-8601 */, kind: String, spec: Option<String>, payload_summary: String }`
- [x] Implementar `#[tauri::command] fn dashboard_token_summary(project_path: String) -> Result<TokenSummary, String>` agregando `events` onde `kind = 'token.saved'`, somando `payload.saved`, agrupando top 5 por `spec`
- [x] Implementar `#[tauri::command] fn dashboard_month_activity(project_path: String, year: i32, month: u32) -> Result<Vec<DayActivity>, String>` — emite uma entrada por dia do mês (1..N) mesmo com 0 eventos; `top_phase` é a fase com mais eventos no dia
- [x] Implementar `#[tauri::command] fn dashboard_events_feed(project_path: String, limit: u32) -> Result<Vec<FeedEvent>, String>` — `ORDER BY ts DESC LIMIT ?`; `payload_summary` é uma string ≤120 chars derivada do payload (ex.: `"draft → implementing"` para `pipeline.status`)
- [x] Em `main.rs`, acrescentar os 3 handlers ao `tauri::generate_handler![...]` (bodies em `spec_views.rs`, registro em `lib.rs::generate_handler!` via `spec_views::…`, marcador textual em `main.rs` para o grep do AC-3)
- [x] `cargo check -p dashboard --manifest-path apps/dashboard/src-tauri/Cargo.toml` (nota: package name real é `mustard-dashboard`; com `-p mustard-dashboard` compila limpo, 0 errors/0 warnings)

## Dependências

Nenhuma — primeira wave, paralela à [[wave-1-badges]].

## Network

- Parent: [[2026-05-20-dashboard-visual-overview]]
- Paralela a: [[wave-1-badges]]
- Desbloqueia: [[wave-2-data]] (consome os 3 commands Tauri via invoke wrappers)
- Memória compartilhada: grava em `events.agent.memory` payload `{commands: [...], structs: [...], notes: "..."}` para [[wave-2-data]] consumir no prompt.

## Limites

Em escopo: `apps/dashboard/src-tauri/src/spec_views.rs`, `apps/dashboard/src-tauri/src/main.rs`.

Fora de escopo: tudo mais (ver `wave-plan.md § Limites globais`).
