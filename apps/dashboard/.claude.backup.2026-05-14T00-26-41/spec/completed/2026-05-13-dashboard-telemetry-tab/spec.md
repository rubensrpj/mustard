# Feature: dashboard-telemetry-tab

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T05:10:00Z
### Lang: pt

## Contexto

O dashboard hoje mostra contagens agregadas (specs, knowledge, eventos, tokens) mas não expõe a value prop principal do Mustard — quanto trabalho os hooks anti-slope e o roteamento de modelo estão poupando, nem como o RTK reduz tokens de CLI. Os dados existem no disco: `.claude/.metrics/*.jsonl` (um arquivo por hook, append-only, com `tokens_saved` por evento) e `rtk gain` (CLI plain text com totals globais e tabela "By Command"). Sem essa visibilidade, o usuário não consegue justificar o custo do scaffold nem identificar quais hooks/comandos rendem mais. A página deve render dois blocos preventivos (`RTK` + `Anti-Slope Hooks`), o roteamento de modelo, o workflow por fase do pipeline e o tool breakdown — tudo dependente do projeto selecionado, já que cada um tem seu próprio `.claude/`. Hoje, sem uma aba dedicada, esses números ficam ilegíveis nos JSONL crus.

## Resumo

Adicionar um command Tauri `dashboard_telemetry(repo_path)` que consolida RTK CLI parsing + hook JSONL counts + SQLite/JSONL fallback para routing/workflow/tool-breakdown; expor via `fetchTelemetry` em `src/lib/dashboard.ts`; criar página `/telemetry` com picker de projeto, cinco seções densas estilo Linear e helpers de formatação em `src/lib/format.ts`.

## Limites

Edições intencionalmente restritas a:

- `src-tauri/src/lib.rs` (novo struct grupo + `#[tauri::command] dashboard_telemetry` + registro em `invoke_handler!`)
- `src-tauri/src/telemetry.rs` (novo módulo — parsers RTK / hook JSONL / routing / workflow / tool breakdown)
- `src-tauri/src/db.rs` (dois novos helpers `workflow_by_phase_from_db` e `tool_breakdown_from_db`)
- `src/lib/dashboard.ts` (interfaces TS + `fetchTelemetry`)
- `src/lib/format.ts` (novo — `formatNumber`, `formatPct`, `formatDurationMs`, `formatTokens`)
- `src/pages/Telemetry.tsx` (novo)
- `src/components/layout/Sidebar.tsx` (NavLink "Telemetry" entre Activity e Knowledge)
- `src/App.tsx` (Route `/telemetry`)

Fora de boundary: outros comandos Tauri, schemas auto-gerados, hooks Mustard (`.claude/hooks/*`), demais páginas/componentes, mustard.json/pipeline-config.md, dependências (não adicionar libs de charting).

## Arquivos

Novos (~3):
- `src-tauri/src/telemetry.rs`
- `src/lib/format.ts`
- `src/pages/Telemetry.tsx`

Modificados (~5):
- `src-tauri/src/lib.rs`
- `src-tauri/src/db.rs`
- `src/lib/dashboard.ts`
- `src/components/layout/Sidebar.tsx`
- `src/App.tsx`

## Component Contract

### `src/pages/Telemetry.tsx`

**Propósito:** Página única que consolida visibilidade preventiva (RTK + hooks anti-slope), roteamento de modelo, workflow do pipeline e tool breakdown, tudo escopado ao projeto selecionado.

**Estado local:**
- `selectedProjectId: string | null` — `useState`, default = `null` (resolve para primeiro projeto do workspace via `useEffect` quando `projects` carrega)

**Dados:**
- `useQuery(['discover', projectsRoot], ...)` reusa o mesmo padrão de Sidebar/Activity para listar projetos
- `useQuery(['telemetry', selectedProject?.path], () => fetchTelemetry(selectedProject!.path), { enabled: !!selectedProject, staleTime: 30_000 })`

**Layout (dense, sem chart libs):**

1. **Header**: breadcrumb `Mustard / Telemetry` + `<h1>Telemetry & Cost Savings — {project?.name ?? 'all projects'}</h1>` + project picker à direita (`<select>` nativo estilizado com tailwind, options = lista de `Project`)
2. **Empty state** (sem projetos / sem projectsRoot): `"Selecione um projeto na sidebar."` em `text-muted-foreground`
3. **Seção `RTK Token Savings`** — se `rtk.available === false`: card cinza único `"RTK não instalado — rode rtk init -g globalmente"`. Se disponível: grid de 4 KPI cards (`total_commands`, `tokens_saved`, `savings_pct`, `avg_time`) + tabela `By Command` (top 10) com colunas `#`, `Command` (mono), `Count`, `Saved` (formatado), `Avg %` (com barra indigo `bg-indigo-500/30` width:`avg_pct`%)
4. **Seção `Anti-Slope Hooks`** — lista densa (Linear-like): cada hook é uma `<li>` com `StatusDot variant="done"` (verde) + `<span className="font-mono">{hook}</span>` + `{fires} fires` + `{formatTokens(tokens_saved)} saved` + `relativeTime(most_recent_ts)` à direita (`ml-auto`). Ordem: descendente por `tokens_saved`
5. **Seção `Model Routing`** — card-resumo: `"{routing.blocks} blocks · {routing.allows} allows · {prevention_rate}% prevention rate"` (calcula `prevention_rate = blocks/(blocks+allows)*100`, formato `formatPct`). Sub-lista por intent (top 5): cada linha `<intent> — {blocks} blocked / {allows} allowed` + mini-bar dupla (vermelho/verde) proporcional
6. **Seção `Pipeline Workflow`** — para cada `PhaseCount` em `workflow.by_phase`: label `phase` + barra `bg-amber-500/30` com `width: ${count/max*100}%` + count à direita. Ordem fixa: `ANALYZE, PLAN, EXECUTE, QA, CLOSE` (renderizar mesmo phases ausentes como `0`)
7. **Seção `Tool Breakdown`** — lista densa top 15: `tool_name` (mono) + count + barra `bg-slate-400/30` proporcional

**Loading state:** Para a query principal, mostrar `<div className="h-6 bg-muted/40 rounded animate-pulse" />` × 5 em cada seção (mesmo padrão de Activity).

**Erros:** Falha no fetch → `<p className="text-[13px] text-destructive">Erro ao carregar telemetry: {error.message}</p>`.

**Sem efeitos colaterais.** Apenas leitura.

## Tarefas

### Backend Agent (Wave 1 — Rust)

- [ ] Criar `src-tauri/src/telemetry.rs` com structs públicos `TelemetrySummary`, `RtkBlock`, `RtkCommandRow`, `MeasuredBlock`, `HookFireCount`, `RoutingBlock`, `RoutingByIntent`, `WorkflowBlock`, `PhaseCount`, `ToolCount` — todos derivam `Serialize` com `#[serde(rename_all = "snake_case")]`, conforme schema na seção Resumo do prompt original (campos exatos: `rtk: RtkBlock { available: bool, total_commands: Option<u64>, input_tokens, output_tokens, tokens_saved, savings_pct: Option<f64>, total_exec_time_ms: Option<u64>, by_command: Vec<RtkCommandRow> }`; `RtkCommandRow { command: String, count: u64, saved: u64, avg_pct: f64 }`; `MeasuredBlock { tokens_total: u64, tokens_today: u64 }`; `HookFireCount { hook: String, fires: u64, tokens_saved: u64, most_recent_ts: Option<String> }`; `RoutingBlock { blocks: u64, allows: u64, by_intent: Vec<RoutingByIntent> }`; `RoutingByIntent { intent: String, blocks: u64, allows: u64 }`; `WorkflowBlock { by_phase: Vec<PhaseCount> }`; `PhaseCount { phase: String, count: u64 }`; `ToolCount { tool_name: String, count: u64 }`)
- [ ] Em `telemetry.rs`, implementar `pub fn rtk_summary(repo_path: &Path) -> RtkBlock`: chama `std::process::Command::new("rtk").arg("gain").current_dir(repo_path).output()`. Se `Err(_)` ou `output.status.success() == false` → retorna `RtkBlock { available: false, total_commands: None, input_tokens: None, output_tokens: None, tokens_saved: None, savings_pct: None, total_exec_time_ms: None, by_command: vec![] }`. Caso OK, parsear `stdout` linha-a-linha com helpers regex (não adicionar `regex` crate — usar `str::find` + `str::strip_prefix` + manual parse). Helper `parse_token_count(s: &str) -> Option<u64>` que aceita `"1234"`, `"1.5K"`, `"120.1M"`, `"3.2G"` e multiplica por 1000^unit. Extrair `total_commands` (linha começando com `"Total commands:"`), `input_tokens` (`"Input tokens:"`), `output_tokens` (`"Output tokens:"`), `tokens_saved` + `savings_pct` (linha `"Tokens saved:    120.1M (79.6%)"` → captura ambos), `total_exec_time_ms` (`"Total exec time:    1368m35s"` → `(min*60+sec)*1000`). Para a tabela `By Command`: detectar header iniciando com `"  #"` ou `"By Command"`; depois, para até 10 linhas seguintes que casam o padrão `^\s*\d+\.\s+`, fazer split por whitespace 2+ caracteres e extrair `command`, `count`, `saved`, `avg_pct`. Linhas que não casarem o padrão → ignorar silenciosamente
- [ ] Em `telemetry.rs`, implementar `pub fn hook_fire_counts(repo_path: &Path) -> Vec<HookFireCount>`: listar `.claude/.metrics/` (se não existir → `vec![]`). Para cada arquivo `*.jsonl` cujo `file_stem` NÃO seja `rtk-gain`, `rtk-rewrite`, nem `budget-observations` (esses três são observacionais/não-preventivos): abrir, percorrer linhas, parsear cada uma como `serde_json::Value` (linhas inválidas: skip). Acumular `fires` (count de linhas válidas), `tokens_saved` (sum de `v["tokens_saved"].as_u64().unwrap_or(0)`), `most_recent_ts` (último `v["ts"].as_str()` visto). Retornar `Vec<HookFireCount>` com `hook` = `file_stem`, ordenado decrescente por `tokens_saved` (depois por `fires` como tie-breaker). Falha de I/O em um arquivo individual → log via `eprintln!` e seguir
- [ ] Em `telemetry.rs`, implementar `pub fn routing_breakdown(repo_path: &Path) -> RoutingBlock`: ler `.claude/.metrics/model-routing-gate.jsonl` (se não existir → `RoutingBlock { blocks: 0, allows: 0, by_intent: vec![] }`). Para cada linha, parsear JSON, classificar: `note == "blocked"` → block; `note` começa com `"allow"` ou `note == "passed"` → allow; outros notes → ignorar para totals mas ainda contar por intent? Não — só counts notes reconhecidas. Group by `payload.intent.as_str()` (intents `None` viram bucket `"unknown"`). Retornar top 5 intents por `blocks + allows` em `by_intent`, e totals globais em `blocks`/`allows`
- [ ] Em `telemetry.rs`, implementar `pub fn workflow_by_phase(repo_path: &Path) -> WorkflowBlock`: tentar SQLite primeiro via `db::with_db(repo_path, db::workflow_by_phase_from_db)`; fallback JSONL: ler `.claude/.harness/events.jsonl`, filtrar `event == "pipeline.phase"`, group by `payload.phase`. Retornar `WorkflowBlock { by_phase: Vec<PhaseCount> }` ordenado decrescente por `count`
- [ ] Em `telemetry.rs`, implementar `pub fn tool_breakdown(repo_path: &Path) -> Vec<ToolCount>`: tentar SQLite via `db::with_db(repo_path, |c| db::tool_breakdown_from_db(c, 15))`; fallback JSONL: ler `events.jsonl`, filtrar `event == "tool.use"`, group by `payload.tool || payload.tool_name`. Retornar top 15
- [ ] Em `telemetry.rs`, implementar `pub fn measured(repo_path: &Path) -> MeasuredBlock`: chamar `db::with_db(repo_path, db::metrics_from_db)` e extrair `tokens_total` / `tokens_today` do resultado. Fallback → `MeasuredBlock { tokens_total: 0, tokens_today: 0 }` (não duplicar lógica JSONL — fora do DB, tokens caem a zero, alinhado ao `dashboard_metrics` atual)
- [ ] Em `src-tauri/src/db.rs`, adicionar dois helpers: (a) `pub fn workflow_by_phase_from_db(conn: &Connection) -> Result<crate::telemetry::WorkflowBlock, String>` rodando `SELECT json_extract(payload, '$.phase') AS phase, COUNT(*) FROM events WHERE event = 'pipeline.phase' GROUP BY phase ORDER BY 2 DESC` (ignorar `phase == NULL`); (b) `pub fn tool_breakdown_from_db(conn: &Connection, limit: usize) -> Result<Vec<crate::telemetry::ToolCount>, String>` rodando `SELECT COALESCE(json_extract(payload, '$.tool'), json_extract(payload, '$.tool_name')) AS tool, COUNT(*) FROM events WHERE event = 'tool.use' GROUP BY tool ORDER BY 2 DESC LIMIT ?1` (ignorar `tool == NULL`)
- [ ] Em `src-tauri/src/lib.rs`: adicionar `mod telemetry;` no topo (junto a `mod discovery;` / `pub mod db;`). Implementar `#[tauri::command] fn dashboard_telemetry(repo_path: String) -> Result<telemetry::TelemetrySummary, String>` que compõe: `let base = PathBuf::from(&repo_path); Ok(TelemetrySummary { rtk: telemetry::rtk_summary(&base), measured: telemetry::measured(&base), prevention: telemetry::hook_fire_counts(&base), routing: telemetry::routing_breakdown(&base), workflow: telemetry::workflow_by_phase(&base), tool_breakdown: telemetry::tool_breakdown(&base) })`. Registrar `dashboard_telemetry` na lista de `tauri::generate_handler![]` do final do arquivo
- [ ] Verificar compilação: `cargo check --manifest-path src-tauri/Cargo.toml` deve passar sem warnings novos

### Frontend Agent (Wave 2 — TS bridge + UI)

**Dependência:** Wave 1 Backend completa (struct `TelemetrySummary` exportada via IPC).

- [ ] Criar `src/lib/format.ts` exportando 4 helpers:
  - `formatNumber(n: number): string` — `< 1_000` → inteiro; `< 1_000_000` → `"1.2K"` (1 decimal, trim `.0`); `< 1_000_000_000` → `"1.5M"`; senão `"2.3G"`. `Number.isFinite` falso → `"0"`
  - `formatTokens(n: number): string` — mesma lógica de `formatNumber` (1 decimal max)
  - `formatPct(p: number): string` — `${p.toFixed(1)}%` (ex: `79.6%`). `Number.isFinite` falso → `"0%"`
  - `formatDurationMs(ms: number): string` — `< 1000` → `"<1s"`; `< 60_000` → `"{Xs}"`; `< 3_600_000` → `"{Xm Ys}"`; senão `"{Hh Ym}"`
- [ ] Em `src/lib/dashboard.ts`, adicionar interfaces TS espelhando os structs Rust (snake_case): `TelemetrySummary`, `RtkBlock`, `RtkCommandRow`, `MeasuredBlock`, `HookFireCount`, `RoutingBlock`, `RoutingByIntent`, `WorkflowBlock`, `PhaseCount`, `ToolCount`. Adicionar `export function fetchTelemetry(repoPath: string): Promise<TelemetrySummary> { return invoke<TelemetrySummary>("dashboard_telemetry", { repoPath }); }`
- [ ] Criar `src/pages/Telemetry.tsx` conforme Component Contract acima. Imports principais: `useState, useEffect, useMemo` do react, `useQuery` do `@tanstack/react-query`, `useStore` de `@/lib/store`, `discoverProjects` de `@/api/discovery`, `fetchTelemetry` + tipos de `@/lib/dashboard`, helpers de `@/lib/format`, `StatusDot` de `@/components/StatusDot`, `Badge` de `@/components/ui/badge`, `relativeTime` de `@/lib/time`. Estrutura: header com breadcrumb + h1 + `<select>` picker (small, `text-[12px]`); cinco `<section>` densas com `text-sm`/`gap-1`/`px-2 py-1`. Tabela RTK usa `<table>` simples com `text-[13px]`. Barras de progresso: `<div className="h-1 bg-muted rounded overflow-hidden"><div className="h-full bg-indigo-500/30" style={{ width: \`${pct}%\` }} /></div>`. Para o picker default-selecionar primeiro projeto: `useEffect(() => { if (!selectedProjectId && projects?.length) setSelectedProjectId(projects[0].id); }, [projects, selectedProjectId])`
- [ ] Em `src/components/layout/Sidebar.tsx`: import `Gauge` de `lucide-react` (somar ao import atual de `Home, Settings, BookOpen, Activity`). Adicionar `<NavLink to="/telemetry" className={navItemClass}><Gauge className="h-3.5 w-3.5" /> Telemetry</NavLink>` **entre** o NavLink existente `to="/activity"` e o Separator imediatamente abaixo (Activity → Telemetry → Separator → Workspace)
- [ ] Em `src/App.tsx`: importar `Telemetry` de `@/pages/Telemetry` e adicionar `<Route path="/telemetry" element={<Telemetry />} />` antes do `<Route path="/settings" .../>`
- [ ] Verificar build: `pnpm run build` deve passar sem erros de tipo

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent. (Header kept in English because `qa-run.js` does not yet parse the PT heading "Critérios de Aceitação".)

- [x] AC-1: Build TypeScript passa — Command: `pnpm run build`
- [x] AC-2: cargo check passa sem warnings novos — Command: `node -e "const{execSync}=require('child_process');const path=require('path');const cargo=path.join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin',process.platform==='win32'?'cargo.exe':'cargo');execSync(JSON.stringify(cargo)+' check --manifest-path src-tauri/Cargo.toml --message-format=short',{stdio:'inherit'})"`
- [x] AC-3: Módulo telemetry.rs existe e declara o struct TelemetrySummary com os 6 blocos — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/telemetry.rs','utf8');process.exit((c.includes('pub struct TelemetrySummary')&&c.includes('rtk:')&&c.includes('measured:')&&c.includes('prevention:')&&c.includes('routing:')&&c.includes('workflow:')&&c.includes('tool_breakdown:'))?0:1)"`
- [x] AC-4: Command dashboard_telemetry registrado em invoke_handler — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const occ=c.split('dashboard_telemetry').length-1;process.exit((c.includes('generate_handler')&&occ>=2)?0:1)"`
- [x] AC-5: TS bridge expõe fetchTelemetry com tipo TelemetrySummary — Command: `node -e "const c=require('fs').readFileSync('src/lib/dashboard.ts','utf8');process.exit((c.includes('fetchTelemetry')&&c.includes('TelemetrySummary')&&c.includes('dashboard_telemetry'))?0:1)"`
- [x] AC-6: src/lib/format.ts criado com os 4 helpers — Command: `node -e "const c=require('fs').readFileSync('src/lib/format.ts','utf8');process.exit((c.includes('formatNumber')&&c.includes('formatPct')&&c.includes('formatDurationMs')&&c.includes('formatTokens'))?0:1)"`
- [x] AC-7: Página Telemetry e rota /telemetry registradas — Command: `node -e "const fs=require('fs');const page=fs.existsSync('src/pages/Telemetry.tsx');const app=fs.readFileSync('src/App.tsx','utf8');process.exit((page&&app.includes('/telemetry')&&app.includes('Telemetry'))?0:1)"`
- [x] AC-8: NavLink Telemetry presente no Sidebar entre Activity e Workspace — Command: `node -e "const c=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8');const iAct=c.indexOf('to=\"/activity\"');const iTel=c.indexOf('to=\"/telemetry\"');const iWS=c.indexOf('Workspace');process.exit((iAct>=0&&iTel>iAct&&iWS>iTel)?0:1)"`

## Preocupações

- WARN (analyze-validation, layer-gap, x2): heurística não detectou extensões `.rs` / `.tsx` nas seções Backend Agent / Frontend Agent. Falso positivo — `src-tauri/src/lib.rs`, `src-tauri/src/telemetry.rs`, `src-tauri/src/db.rs`, `src/pages/Telemetry.tsx`, `src/lib/format.ts` e `src/lib/dashboard.ts` estão listados em `## Arquivos`. Sem ação.

## Não-Objetivos

- Não adicionar dependência de chart library (Recharts, Chart.js, D3) — barras são `<div>` com `width:%` puro
- Não adicionar polling agressivo / live updates / watcher de filesystem — Wave C cuida disso
- Não persistir `selectedProjectId` no Zustand store — local `useState` resolve; persistência fica para refinamento futuro
- Não adicionar paginação ou ordenação interativa nas tabelas RTK / Anti-Slope / Tool Breakdown — fixo, sort no backend
- Não adicionar export CSV/JSON dos números — só visualização
- Não tocar em outras páginas (Home/Activity/Knowledge/ProjectDetail/Settings) — só os 3 arquivos modificados (lib.rs, db.rs, dashboard.ts, Sidebar.tsx, App.tsx)
- Não criar uma tabela `events_by_phase` ou índices novos em SQLite — usar `json_extract` direto
- Não migrar `dashboard_metrics` para dentro de `dashboard_telemetry` — `MeasuredBlock` chama o helper compartilhado mas o command antigo permanece
- Não adicionar Commands catalog, PRD builder, Glossary, license editor (mencionados no prompt original como Wave D/legacy bonus) — fora de escopo
