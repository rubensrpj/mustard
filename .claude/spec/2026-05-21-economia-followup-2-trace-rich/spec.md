# Followup-2 — Trace rico + Specs filter + Performance + Economia union

### Parent: [[2026-05-20-economia-moat-unification]]
### Status: completed
### Phase: EXECUTE
### Scope: full (tactical-fix bundle ampliado)
### Checkpoint: 2026-05-21T11:50:00Z
### Lang: pt

## PRD

Round 2 de followup do parent identificou 4 problemas concretos via uso real do dashboard. Round 1 endereçou parcialmente (visual, filter básico, --once flag) mas faltou diagnóstico arquitetural:

1. **Specs page lista terminais como "Ativas"** — filtro frontend não cruza `status` com `phase`. Status `completed`/`closed-followup`/`cancelled` vaza.
2. **Trace só mostra payload bruto** — events `tool.use` são PreToolUse (intenção: `{tool, target: {command, file?, description}}`). Sem `tool.result` event em PostToolUse, stdout/diff/content nunca chegam ao banco. ToolEventRow renderiza JSON cru porque os campos `before/after/stdout/content` que ele esperava não existem no DB.
3. **Performance ruim** — falta índice composto `events(spec, event)` (mais usado em `dashboard_spec_trace`). Plus React Query sem `staleTime` → toda navegação refaz fetch.
4. **Economia em zero apesar de 22598 frames ingest** — `transcript::ingest` retorna `ApiCostFrame` (alias semântico de SpanRecord), persistido em tabela `api_cost_frames` separada. Card "CUSTO ANTHROPIC" lê só tabela `spans`. Schema split silencioso.

## Métrica de sucesso

- Page Specs tab "Ativas" NUNCA lista spec com status `completed`/`closed-followup`/`cancelled`.
- Trace tab em uma spec real renderiza: comando completo + stdout do Bash, file path + diff visual do Edit/Write, file path + content preview do Read. NÃO mais JSON cru.
- Dashboard navegação spec→spec ou tab→tab fica perceptivelmente mais rápida (<200ms vez de >1s).
- Economia card "CUSTO ANTHROPIC" mostra > $0 e contagem refletindo a união `spans` ∪ `api_cost_frames`.

## Não-Objetivos

- Não construir novo painel/visualização — só consertar o que existe.
- Não migrar dados velhos (frames pré-followup-2 ficam como estão).
- Não adicionar suporte OTLP/gRPC (continua JSON local).
- Não tipar `tool_use_id` em `SpanRecord` (continua via `extra` map, W4 Concern).

## Acceptance Criteria

- [x] AC-1: Build dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-3: cargo check rt + core passam — Command: `cargo check -p mustard-rt -p mustard-core`
- [x] AC-4: tool_result hook existe — Command: `node -e "if(!require('fs').existsSync('apps/rt/src/hooks/tool_result.rs'))throw new Error('hook missing')"`
- [x] AC-5: tool_result registrado no dispatcher — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/hooks/mod.rs','utf8');if(!t.includes('tool_result'))throw new Error('module not wired in mod.rs')"`
- [x] AC-6: `dashboard_spec_trace` joinou tool.result — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry.rs','utf8');if(!/tool\\.result|load_tool_results/.test(t))throw new Error('telemetry not joining tool.result events')"`
- [x] AC-7: Migration v5 com índice events(spec,event) — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/store/migrations.rs','utf8');if(!/idx_events_spec_event|migrate_v4_to_v5/.test(t))throw new Error('migration v5 missing index')"`
- [x] AC-8: Economia reader inclui api_cost_frames — Command: `node -e "const t=require('fs').readFileSync('packages/core/src/economy/reader.rs','utf8');if(!/api_cost_frames|UNION ALL.*spans/s.test(t))throw new Error('reader still spans-only')"`
- [x] AC-9: Specs page filter cruza status terminais — Command: `node -e "const fs=require('fs');const candidates=['apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/components/specs/SpecList.tsx','apps/dashboard/src/hooks/useSpecsFiltered.ts'];const found=candidates.some(p=>{try{const t=fs.readFileSync(p,'utf8');return /completed|closed.followup|cancelled/.test(t)&&/(filter|isTerminal|exclude)/i.test(t)}catch{return false}});if(!found)throw new Error('terminal filter not found in any specs page/hook')"`
- [x] AC-10: cargo test core + rt — Command: `cargo test -p mustard-core -p mustard-rt`

## Plano

4 sub-tarefas em ordem:

### 4a. Migration v5 + índices (≈30 LOC, packages/core)
`packages/core/src/store/migrations.rs`: APPEND migration v5. Adicionar:
- `CREATE INDEX IF NOT EXISTS idx_events_spec_event ON events(spec, event)` (composite p/ `dashboard_spec_trace` queries)
- `CREATE INDEX IF NOT EXISTS idx_events_actor_event ON events(actor_id, event)` (p/ agent join)
- (opcional) `CREATE INDEX IF NOT EXISTS idx_api_cost_frames_project_ts ON api_cost_frames(project_path, ts)` se ainda não existir.
Bumpa `LATEST_VERSION` para 5.

### 4b. Hook `tool_result` (~150 LOC, apps/rt)
Novo arquivo `apps/rt/src/hooks/tool_result.rs`:
- Implementa trait `HookModule` para evento `PostToolUse` (matcher `.*` ou listed tools).
- Lê do payload do hook: `tool_name`, `tool_input` (já tem do PreToolUse equivalente), `tool_response` (contém stdout/stderr/exit/file_diff dependendo do tool).
- Para `Bash`: extrai `stdout`, `stderr`, `exit_code` do `tool_response.output`.
- Para `Edit`/`Write`/`MultiEdit`: payload tem `file_path` + (`old_string`/`new_string`) ou `content`.
- Para `Read`: payload tem `file_path` + `content` (excerpt).
- Emite evento `tool.result` via `mustard_core::store::EventSink` com payload:
  ```json
  {
    "tool_use_id": "...",
    "tool": "Bash",
    "file_path": "...",
    "stdout_excerpt": "primeiros 2KB",
    "stderr_excerpt": "primeiros 1KB",
    "exit_code": 0,
    "file_before": "...",  // para Edit, raw
    "file_after": "...",   // para Edit, raw
    "content_excerpt": "..." // para Read, primeiros 4KB
  }
  ```
- Tamanho cap: excerpts truncados em 2-4KB pra não inflar a tabela.
- Registrar em `apps/rt/src/hooks/mod.rs`: `pub mod tool_result; impl Dispatcher::register(... tool_result ...);` no slot existente.
- Test inline: simular payload Bash → verificar emissão correta.

### 4c. Backend `dashboard_spec_trace` joina tool.result (~80 LOC, apps/dashboard/src-tauri)
`apps/dashboard/src-tauri/src/telemetry.rs::load_tool_events`:
- Modificar query para também carregar `tool.result` events: `SELECT ts, actor_id, payload, event FROM events WHERE spec = ?1 AND event IN ('tool.use', 'tool.result') ORDER BY id`.
- No `build_trace_tree`, para cada `tool.use`, encontrar matching `tool.result` por `tool_use_id` (se tool_use_id existir no payload de ambos) ou por ordem cronológica (pair `tool.use[N]` com `tool.result[N]`).
- Merge: `ToolEvent.payload` ganha campos `result` opcional com o conteúdo do `tool.result`.
- Struct `ToolEvent` ganha `result: Option<ToolResultPayload>`.

### 4d. Frontend `ToolEventRow` renderiza shape real (~80 LOC, apps/dashboard/src)
`apps/dashboard/src/components/trace/ToolEventRow.tsx`:
- Atualizar `Payload` type para refletir o shape real: `{tool, target: {command, file_path?, description}, phase}` + opcional `result: {stdout_excerpt, stderr_excerpt, exit_code, file_before, file_after, content_excerpt}`.
- `tool=Bash`: header `$ {target.command}`; se `result.stdout_excerpt` existir, `<CodeBlock code={result.stdout_excerpt} lang="plain" />`; se `result.stderr_excerpt`, segundo bloco vermelho.
- `tool=Edit|Write|MultiEdit`: header `{target.file_path}`; se `result.file_before` E `result.file_after`, `<DiffViewer before={result.file_before} after={result.file_after} mode="split" />`; senão, fallback "comando registrado mas resultado ainda não capturado".
- `tool=Read`: header `{target.file_path}`; se `result.content_excerpt`, `<CodeBlock code={result.content_excerpt} lang={detectLang(target.file_path)} showLineNumbers />`. Se ext=`.md`, opcionalmente render via `react-markdown` (já dep do projeto).
- Eliminar `payload.before`/`payload.after` legacy se referenciado.

### 4e. Specs page filter + Economia union (~60 LOC, apps/dashboard)
**Specs page:**
- Localizar componente da página Specs (`apps/dashboard/src/pages/Specs.tsx` ou similar). Identificar onde tab "Ativas" filtra a lista.
- Adicionar filtro: spec com `status` terminal (`completed`, `closed-followup`, `cancelled`) NÃO aparece em "Ativas". Aparece em "Encerradas".
- Reusar o helper `is_terminal_status` se já houver no frontend, ou criar inline.

**Economia union:**
- `packages/core/src/economy/reader.rs::economy_summary`: a query base de spans precisa ser `SELECT ... FROM (SELECT ... FROM spans UNION ALL SELECT ... FROM api_cost_frames)`.
- Garantir que colunas alinhem (input_tokens, output_tokens, cost_usd_micros, ts, etc.).
- Mesmo treatment em `per_agent_costs`, `per_spec_costs`, `per_wave_costs` se aplicável.
- Adicionar 1 teste em `tests/economy_basic.rs` ou `economy_attribution.rs`: `test_economy_summary_includes_api_cost_frames`.

### 4f. React Query staleTime (~5 LOC, apps/dashboard)
`apps/dashboard/src/main.tsx` ou `App.tsx`: localizar `QueryClient` instance. Setar `defaultOptions.queries.staleTime = 60_000` (1 min) + `refetchOnWindowFocus = false` se não estiver.

## Informações da Entidade

- Nova event kind: `tool.result` (vai pra tabela `events` existente, sem schema nova)
- Sem entidade Rust nova; `ToolResultPayload` é struct serde inline em telemetry.rs

## Arquivos (~10)

```
packages/core/src/store/migrations.rs           (modify — APPEND v5: idx_events_spec_event, idx_events_actor_event)
packages/core/src/economy/reader.rs             (modify — economy_summary UNION ALL spans + api_cost_frames)
packages/core/tests/economy_basic.rs            (modify — adicionar test_economy_summary_includes_api_cost_frames)
apps/rt/src/hooks/tool_result.rs                (new — emit tool.result event in PostToolUse)
apps/rt/src/hooks/mod.rs                        (modify — registrar tool_result module)
apps/dashboard/src-tauri/src/telemetry.rs       (modify — load_tool_events joina tool.use+tool.result; ToolEvent.result)
apps/dashboard/src/lib/types/trace.ts           (modify — opcionalmente ToolResult shape)
apps/dashboard/src/components/trace/ToolEventRow.tsx (modify — render por shape real com result)
apps/dashboard/src/pages/Specs.tsx              (modify — filtrar terminais em "Ativas") OR
apps/dashboard/src/components/specs/{SpecList,specs-page-filter}.tsx (idem se não em Specs.tsx)
apps/dashboard/src/main.tsx                     (modify — QueryClient defaultOptions staleTime 60s)
```

## Tarefas

### Backend Agent (4a + 4b + 4c)
- [ ] Migration v5 (idx_events_spec_event + idx_events_actor_event)
- [ ] Hook tool_result.rs registrando PostToolUse → emit event `tool.result` com payload truncado
- [ ] Wire mod.rs + Dispatcher
- [ ] dashboard_spec_trace joinar tool.use+tool.result por tool_use_id (com fallback cronológico)
- [ ] cargo test passar

### Library Agent (4e Economia union)
- [ ] economy::reader::economy_summary UNION ALL spans + api_cost_frames
- [ ] per_agent_costs, per_spec_costs, per_wave_costs idem se aplicável
- [ ] test_economy_summary_includes_api_cost_frames inline em tests/economy_basic.rs
- [ ] cargo test -p mustard-core verde

### Frontend Agent (4d + 4e + 4f)
- [ ] ToolEventRow renderiza Bash stdout, Edit/Write diff, Read content preview baseado em payload.result
- [ ] Pages/Specs filter terminais em "Ativas"
- [ ] QueryClient staleTime 60s + refetchOnWindowFocus false em main.tsx
- [ ] pnpm build + tsc verdes

## Dependências

- Parent: [[2026-05-20-economia-moat-unification]] (closed-followup)
- Followup-1: [[2026-05-21-economia-moat-followup-fixes]] (completed)
- mustard-rt binário já reinstalado em 2026-05-21T07:25
- Após implementação: reinstalar `cargo install --path apps/rt --force` pra hook `tool_result` ativar

## Limites

Em escopo: arquivos listados em `## Arquivos`. Migration v5 APPEND-only.

Fora de escopo:
- Backfill de eventos `tool.result` antigos (só novos a partir do hook ativo)
- Tabela separada `tool_results` (usa events existente)
- Schema change em `spans`/`api_cost_frames`
- Tipar `tool_use_id` em SpanRecord (debt W4)
- Virtualização do componente Trace (atual scroll é suficiente para escala <1k events)
