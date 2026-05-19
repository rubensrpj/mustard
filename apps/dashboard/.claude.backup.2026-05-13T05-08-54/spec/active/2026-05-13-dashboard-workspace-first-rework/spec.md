# Feature: Dashboard Workspace-First Rework

### Status: draft | Phase: PLAN | Scope: full
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

O Mustard Dashboard expõe hoje a barra lateral como uma lista de projetos com badge "Workspace", mas ela só age como link de navegação — clicar em um projeto abre `/project/{id}` sem que páginas como Home, Activity, Knowledge ou Telemetry recebam esse contexto. O resultado é uma UI que mostra metadado de evento (linhas de JSONL recentes, contagem total de specs) em vez de produto (Pass@1 desta workspace, lead time médio, gargalos de fix-loop). Cada página fala um dialeto: Telemetry tem dropdown próprio governado por um `useState` local, Knowledge entra em modo search-first vazio, Activity é fila crua de eventos. Há ainda duplicação visível na Home — "Em execução" e "Pipelines Ativas" coexistem como duas seções repetidas — e a tipografia mistura corpos pequenos (`html { font-size: 13px }`) com cabeçalhos pouco diferenciados. A consequência é que o operador não consegue dizer rapidamente "como está minha workspace agora" sem cruzar três telas. Esta rework introduz `activeWorkspaceId` como primitiva global, faz a sidebar virar seletor (não rota), e cada página passa a renderizar narrativa rica a partir do schema Phase 1 já presente (`spans`, `events_fts`, `knowledge`).

## Resumo

Adicionar `activeWorkspaceId` ao Zustand store, transformar a sidebar em seletor de workspace e remodelar Home/Activity/Knowledge/Quality/Telemetry + tipografia para renderizar narrativa escopada por workspace a partir do schema SQL Phase 1 existente. Inclui 3 funções puras novas em `db.rs`, 3 commands Tauri, 2 índices novos e uma página Quality inédita.

## Limites

- NÃO refatorar `discovery.rs` ou `watcher.rs`.
- NÃO mexer em `ProjectDetail.tsx` ou `SpecDetail.tsx`.
- NÃO mexer em `CommandPalette.tsx` além de adicionar o action group "Switch workspace".
- NÃO introduzir novas dependências (npm ou cargo).
- NÃO criar novas tabelas SQL; apenas dois `CREATE INDEX IF NOT EXISTS`.
- NÃO assumir que `metrics_projection` existe (não existe hoje — ver Preocupações). Quality fns retornam vazio quando schema Phase 1 ausente.
- Queries agregadas devem ficar `<100ms` em DB com ~10k events; índices novos fazem parte do escopo.

## Files (~13)

- `src/lib/store.ts` — adicionar `activeWorkspaceId: string | null` + `setActiveWorkspaceId` persistido (store atual nas linhas 4-25).
- `src/components/layout/Sidebar.tsx` — projetos viram botões que chamam `setActiveWorkspaceId(project.id)` em vez de `NavLink`; indicador visual do workspace ativo; novo nav link "Quality".
- `src/components/layout/Topbar.tsx` — renderizar nome da workspace ativa + atalho "Switch (Cmd+K)"; placeholder "Choose a workspace" quando null.
- `src/components/CommandPalette.tsx` — adicionar action group "Switch workspace" listando todos os projetos descobertos; seleção chama `setActiveWorkspaceId`.
- `src/pages/Home.tsx` — modo dual: sem workspace → Portfolio (cards por projeto com mini-KPIs); com workspace → grid de KPIs reais (lead time avg, Pass@1, tokens 7d, throughput specs/dia 30d); colapsar "Em execução" + "Pipelines Ativas" em seção única "Active pipelines" (atualmente duplicadas em `Home.tsx:73-74` e `Home.tsx:96`).
- `src/pages/Activity.tsx` — Tabs (Timeline | Raw); Timeline consome `dashboard_activity_aggregated`; Raw mantém lista crua.
- `src/pages/Knowledge.tsx` — browse-first; query inicial via `dashboard_knowledge_browse` agrupada por tipo; search vira refinamento; empty state instrui `/mustard:knowledge`.
- `src/pages/Telemetry.tsx` — remover `useState` local `selectedProjectId` e `<select>` (linhas 21, 70-78); ler `activeWorkspaceId` do store; placeholder quando null.
- `src/pages/Quality.tsx` (NEW) — Pass@1 global e por role, fix-loop rate, top 5 waves mais lentas, avg input/output tokens por phase.
- `src/App.tsx` — registrar rota `/quality` → `<Quality />`.
- `src-tauri/src/db.rs` — adicionar `add_phase1_indexes(conn)` idempotente, e 3 fns puras: `aggregate_activity_from_db`, `quality_metrics_from_db`, `knowledge_browse_from_db`.
- `src-tauri/src/lib.rs` — 3 commands novos: `dashboard_activity_aggregated`, `dashboard_quality_metrics`, `dashboard_knowledge_browse`; registrar em `generate_handler!`.
- `src/style.css` — base `13px` → `15px` (linha 131); tokens `--text-h1: 24px`, `--text-h2: 20px`, `--text-h3: 17px`, `--text-body: 15px`, `--text-code: 14px`, `--leading-body: 1.6`; aplicar em `@layer base` para h1/h2/h3/p/code.
- `src/components/Markdown.tsx` — usar tokens; `pre`/`code` com bg `--muted`; blockquote `border-l-2 border-border pl-4 italic`; prose `max-w-[720px]`; mono `JetBrains Mono 14px`.

## Component Contract — Quality.tsx (novo)

- **Props**: `workspacePath: string` (resolvido pelo parent a partir de `activeWorkspaceId`).
- **Estados**: `loading`, `empty` (sem spans/eventos), `error` (Tauri rejeitou), `populated`.
- **Seções**:
  1. **Top metrics row** — Pass@1 (badge %), fix-loop rate (%), avg phase duration (ms).
  2. **Per-role table** — colunas: role, Pass@1, fix-loops, samples.
  3. **Slowest waves** — top 5 ordenado por `duration_ms` DESC; mostra spec, wave, duração.
  4. **Tokens by phase** — bar/list horizontal: phase, input_avg, output_avg.
- **Shape de dados** (resposta do command `dashboard_quality_metrics`):
  ```ts
  type QualityMetrics = {
    pass_at_1: number;          // 0..1
    fix_loop_rate: number;      // 0..1
    avg_phase_duration_ms: number;
    by_role: Array<{ role: string; pass_at_1: number; fix_loops: number; samples: number }>;
    slowest_waves: Array<{ spec: string; wave: number; duration_ms: number }>;
    tokens_by_phase: Array<{ phase: string; input_avg: number; output_avg: number }>;
  };
  ```
- **Empty**: renderizar guia textual — "No span data yet — pipelines need to run with Mustard ≥ Phase 1."
- **Error**: Toast com botão Retry; não propagar exceção. Nunca usar `data!` sem null-check (lição `useeffect_render_race`).
- **Boundary**: não usa `selectedProjectId`; lê `activeWorkspaceId` do store.

## Tasks

### Database Agent (Wave 1)
- [ ] Adicionar `add_phase1_indexes(conn)` idempotente em `src-tauri/src/db.rs` com `CREATE INDEX IF NOT EXISTS idx_events_spec_ts ON events(spec, ts DESC)` e `CREATE INDEX IF NOT EXISTS idx_spans_spec_phase ON spans(spec, phase)`; chamar a partir do bootstrap após `has_phase1_schema()`.
- [ ] Adicionar fn pura `aggregate_activity_from_db(conn, limit) -> Vec<ActivityGroup>` — agrupar `events` por `(spec, wave, payload.action_kind, bucket = ts/300)`; agregar `count`, `min_ts`, `max_ts`, `sum(tokens_saved)` via LEFT JOIN `spans` no mesmo `spec`+`wave`, e `files_touched` via `COUNT(DISTINCT json_extract(payload,'$.target.file'))`. Retornar `vec![]` quando `has_phase1_schema()` é false.
- [ ] Adicionar fn pura `quality_metrics_from_db(conn) -> QualityMetrics` — Pass@1 computado de `events` (specs que atingem `phase='CLOSE'` sem registro intermediário de `escalation` REJECTED|BLOCKED); fix-loop rate via contagem de REJECTED por spec; `by_role` agregado de `spans` (group by `actor.id`); slowest waves via `SELECT spec, wave, duration_ms FROM spans ORDER BY duration_ms DESC LIMIT 5`; `tokens_by_phase` via avg input/output tokens em `spans` agrupado por `phase`. Retornar default vazio quando schema ausente.
- [ ] Adicionar fn pura `knowledge_browse_from_db(conn, limit) -> Vec<KnowledgeRow>` — `SELECT * FROM knowledge ORDER BY type ASC, last_seen DESC LIMIT ?1` (default 500). Retornar `vec![]` quando schema ausente.
- [ ] Adicionar testes unitários (opcional, se houver suite Rust) ou apenas garantir que `cargo check` passa.
- [ ] Validar build: `cargo check --manifest-path src-tauri/Cargo.toml`.

### Backend Agent (Wave 1, blocked by Database)
- [ ] `#[tauri::command] fn dashboard_activity_aggregated(repo_path: String, limit: Option<usize>) -> Result<Vec<ActivityGroup>, String>` — abre conexão via `db::open_connection(&repo_path)`, chama `aggregate_activity_from_db`.
- [ ] `#[tauri::command] fn dashboard_quality_metrics(repo_path: String) -> Result<QualityMetrics, String>` — chama `quality_metrics_from_db`.
- [ ] `#[tauri::command] fn dashboard_knowledge_browse(repo_path: String, limit: Option<usize>) -> Result<Vec<KnowledgeRow>, String>` — chama `knowledge_browse_from_db`.
- [ ] Registrar os 3 em `tauri::generate_handler![..., dashboard_activity_aggregated, dashboard_quality_metrics, dashboard_knowledge_browse]`.
- [ ] Validar build: `cargo check --manifest-path src-tauri/Cargo.toml`.

### Frontend Agent (Wave 2, blocked by Backend) — Store + Shell
- [ ] `src/lib/store.ts`: adicionar `activeWorkspaceId: string | null` (default null) + setter; manter persistência no mesmo `name: 'mustard-dashboard-store'`.
- [ ] `src/components/layout/Sidebar.tsx`: trocar `NavLink to="/project/{id}"` por `<button>` que chama `setActiveWorkspaceId(project.id)`; visual `aria-current="true"` quando `activeWorkspaceId === project.id`; adicionar nav link "Quality" abaixo dos existentes.
- [ ] `src/components/layout/Topbar.tsx`: ler `activeWorkspaceId`, derivar `activeProject` a partir de `useProjects()`; renderizar nome + hint "Switch (⌘K)"; renderizar "Choose a workspace" quando null.
- [ ] `src/components/CommandPalette.tsx`: adicionar grupo "Switch workspace" com um item por projeto; ação executa `setActiveWorkspaceId(p.id)` e fecha o palette. (parallel-safe com Topbar/Sidebar)
- [ ] Type-check: `pnpm tsc --noEmit`.

### Frontend Agent (Wave 3, blocked by Wave 2) — Pages
- [ ] `src/pages/Home.tsx`: ler `activeWorkspaceId`; **null** → Portfolio (cards por projeto com mini-KPIs derivados de `dashboard_metrics` em paralelo via `useQueries`); **set** → grid de KPIs reais (lead time avg, Pass@1, tokens 7d, specs/day 30d) chamando `dashboard_quality_metrics` + agregação local; remover bloco duplicado "Em execução" (linhas 73-74) — manter apenas uma seção "Active pipelines" alimentada por `fetchActivePipelines`. (parallel-safe)
- [ ] `src/pages/Activity.tsx`: Tabs (Timeline | Raw); Timeline consome `dashboard_activity_aggregated`; cards exibem spec, wave, action_kind, N actions, range temporal, tokens saved, files touched; Raw mantém lista atual sem alteração. (parallel-safe)
- [ ] `src/pages/Knowledge.tsx`: na montagem, `useQuery` chama `dashboard_knowledge_browse(activeWorkspaceId)`; renderizar agrupado por tipo (patterns, conventions, entities, lessons, decisions); search input no topo aplica filtro client-side ou re-chama `dashboard_search_knowledge` quando >2 chars; empty state instrui rodar `/mustard:knowledge`. (parallel-safe)
- [ ] `src/pages/Telemetry.tsx`: remover `useState selectedProjectId` (linha 21), remover `<select>` (linhas 70-78); ler `activeWorkspaceId` do store; quando null, render placeholder "Choose a workspace from the sidebar". (parallel-safe)
- [ ] `src/pages/Quality.tsx` (NEW): export default; `useQuery` chama `dashboard_quality_metrics(workspacePath)`; renderiza per Component Contract; usa null-guard explícito antes de acessar `data?.pass_at_1`.
- [ ] `src/App.tsx`: adicionar `<Route path="/quality" element={<Quality />} />` e import.
- [ ] Type-check: `pnpm tsc --noEmit` e lint: `pnpm lint`.

### Frontend Agent (Wave 4, blocked by Wave 3) — Typography
- [ ] `src/style.css`: alterar `html { font-size: 13px }` para `15px` (linha 131); adicionar tokens em `:root` e `.dark`: `--text-h1: 24px`, `--text-h2: 20px`, `--text-h3: 17px`, `--text-body: 15px`, `--text-code: 14px`, `--leading-body: 1.6`; aplicar via `@layer base { h1 { font-size: var(--text-h1); }; h2 { font-size: var(--text-h2); }; h3 { font-size: var(--text-h3); }; body { line-height: var(--leading-body); }; code, pre { font-family: var(--font-mono); font-size: var(--text-code); } }`.
- [ ] `src/components/Markdown.tsx`: classes ajustadas — `pre` com `bg-muted rounded p-3 overflow-x-auto`; `code` inline com `bg-muted px-1 rounded text-[14px] font-mono`; `blockquote` com `border-l-2 border-border pl-4 italic text-muted-foreground`; container raiz com `prose max-w-[720px] leading-relaxed`.
- [ ] Visual check: rodar `pnpm tauri dev` e capturar screenshots das 5 telas pedidas (Home s/ workspace, Home c/ Mustard, Home c/ sialia, Activity Timeline, Knowledge browse, Quality).

### Review Agent (Wave 5, blocked by Wave 4)
- [ ] Checklist-7: guards (fail-open onde aplicável), karpathy (surgical, sem over-engineering), zero novas deps, todas as 3 fns DB retornam vazio quando Phase 1 ausente, nenhum `data!` sem null-check (lição `useeffect_render_race`), Sidebar sem `NavLink` para projetos, Telemetry sem `<select>` próprio.
- [ ] Rodar ACs cross-platform e reportar.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [ ] AC-1: Rust foundation compiles — Command: `cargo check --manifest-path src-tauri/Cargo.toml`
- [ ] AC-2: TypeScript passes strict mode — Command: `pnpm tsc --noEmit`
- [ ] AC-3: Lint passes — Command: `pnpm lint`
- [ ] AC-4: Store exposes activeWorkspaceId + setter — Command: `node -e "const s=require('fs').readFileSync('src/lib/store.ts','utf8'); process.exit(s.includes('activeWorkspaceId') && s.includes('setActiveWorkspaceId') ? 0 : 1)"`
- [ ] AC-5: All 3 new Tauri commands registered — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8'); const need=['dashboard_activity_aggregated','dashboard_quality_metrics','dashboard_knowledge_browse']; process.exit(need.every(n => s.includes('fn '+n)) ? 0 : 1)"`
- [ ] AC-6: SQL indexes created — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/db.rs','utf8'); process.exit(s.includes('idx_events_spec_ts') && s.includes('idx_spans_spec_phase') ? 0 : 1)"`
- [ ] AC-7: Telemetry no longer owns workspace selector — Command: `node -e "const s=require('fs').readFileSync('src/pages/Telemetry.tsx','utf8'); const hasOwnSelect = /<select[\s\S]{0,200}selectedProjectId/.test(s); const hasOwnState = /useState[^;]{0,80}selectedProjectId/.test(s); process.exit(hasOwnSelect || hasOwnState ? 1 : 0)"`
- [ ] AC-8: Quality page exists and is routed — Command: `node -e "const fs=require('fs'); const q=fs.existsSync('src/pages/Quality.tsx'); const app=fs.readFileSync('src/App.tsx','utf8'); process.exit(q && /Quality/.test(app) && /\/quality/.test(app) ? 0 : 1)"`
- [ ] AC-9: Typography base lifted to 15px — Command: `node -e "const s=require('fs').readFileSync('src/style.css','utf8'); process.exit(/html[^{]*\{[^}]*font-size:\s*15px/.test(s) ? 0 : 1)"`

## Preocupações

- **`metrics_projection` não existe**: o brief original referenciou a tabela como fonte de Pass@1, mas o schema atual (`src-tauri/src/db.rs`) tem apenas `events`, `specs`, `knowledge`, `spans`. Resolução: computar Pass@1 e fix-loop rate diretamente da tabela `events` (pipelines que alcançam `phase='CLOSE'` sem `escalation` REJECTED/BLOCKED) + `spans` para durações. Se um dia o projection passar a existir, é refactor isolado.
- **Índices ausentes**: `idx_events_spec_ts` e `idx_spans_spec_phase` foram referenciados como existentes, mas nenhum `CREATE INDEX` foi encontrado em `db.rs`. Spec adiciona como deliverable da Wave 1.
- **Race useEffect/render** (lição `feedback_useeffect_render_race`): páginas novas (Quality especialmente) devem usar null-check explícito antes de `data?.field` e resetar state em re-fetch. Review agent verifica.
- **AC cross-shell** (lição `feedback_ac_shell_portability`): comandos usam `node -e` portável; evitamos sintaxe bash-only (`[`, `for`).
- **Wave decomposition não rodada**: `scope-decompose.js` não foi invocado pois o brief do usuário já chega em forma de spec única; revisitar se EXECUTE surgir >12 arquivos.
- **`Lang=pt` mas QA usa header EN**: `qa-run.js` matcheia `## Acceptance Criteria` literal em inglês — header mantido em EN propositalmente (lição `feedback_qa_run_pt_headers`).
- **Comando `dashboard_search_knowledge`**: já existe e continua sendo usado para refinamento; nada removido.
- **[analyze-validation]** WARN `layer-gap`: spec declara Database Agent mas Files não tem extensões `.sql` — falso positivo, a camada DB é Rust (`db.rs`), não SQL puro.
- **[analyze-validation]** WARN `missing-file`: `src/pages/Quality.tsx` é arquivo NOVO (`NEW` indicado no `## Files`) — não pode existir antes do EXECUTE.

## Não-Objetivos

- Não introduzir tabela `metrics_projection` ou outro schema além dos dois índices.
- Não redesenhar `ProjectDetail.tsx`, `SpecDetail.tsx`, `Settings.tsx` (exceto registrar rota `/quality` em `App.tsx`).
- Não refatorar `watcher.rs`, `discovery.rs` ou hooks do Mustard.
- Não criar agentes/hooks novos.
- Não introduzir testes Playwright na Wave 1 (screenshots manuais cobrem o ciclo de validação).

## Dependencies

- Wave 1 Backend depende de Wave 1 Database (commands chamam fns puras).
- Wave 2 Shell depende de Wave 1 Backend (Topbar/Home consumirão dados).
- Wave 3 Pages depende de Wave 2 (todas leem `activeWorkspaceId` do store).
- Wave 4 Typography é paralela a Wave 3 em teoria, mas spec mantém sequencial para isolar regressões visuais.
- Wave 5 Review final.
