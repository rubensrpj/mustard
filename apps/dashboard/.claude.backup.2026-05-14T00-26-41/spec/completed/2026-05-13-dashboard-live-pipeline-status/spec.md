# Feature: dashboard-live-pipeline-status

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T06:15:00Z
### Lang: pt

## Contexto

Hoje o dashboard é um snapshot: cada `useQuery` tem `staleTime` de 30-60s e só re-fetch em focus/manual reload, então quando o Mustard core avança uma pipeline ativa de `PLAN → EXECUTE → QA → CLOSE` o usuário só descobre depois de clicar em "reload" no Topbar ou trocar de aba. O legacy dashboard tinha endpoint SSE `/api/spec/live` que empurrava transições; em Tauri sem servidor HTTP, o equivalente é um filesystem watcher escutando `.claude/.harness/events.jsonl`, `.claude/.pipeline-states/` e `.claude/spec/active/` e emitindo um evento Tauri que o frontend usa pra invalidar as queries certas. Adicionalmente, falta a noção visual de "o que está rodando agora" — pipeline-states em disco têm wave/phase/tasks/lastDispatchFailure ricos que ninguém renderiza. Sem esses dois, o dashboard passa a impressão de estar morto quando na verdade o core está trabalhando em background.

## Resumo

Adicionar um filesystem watcher em Rust (`notify` crate, debounce 200ms) que emite `dashboard:fs-change` por kind (`events` | `pipeline-state` | `spec`); um command `dashboard_active_pipelines(repo_path)` que enumera pipeline-states vivas com agregados de tasks/wave/failure; um módulo TS `watcher.ts` que assina o evento e invalida as queries afetadas; um componente compartilhado `LivePipelineCard`; e integrar nas páginas Home (nova section "Em execução"), ProjectDetail (cards inline acima das tabs), SpecDetail (refresh ativo + indicador de freshness) e Topbar (indicador "live" pulsante quando há pipelines ativas).

## Limites

Edições intencionalmente restritas a:

- `src-tauri/Cargo.toml` (add `notify` + `notify-debouncer-mini`)
- `src-tauri/src/watcher.rs` (novo — `WatcherState`, `ensure_watching`, classificador de kind)
- `src-tauri/src/lib.rs` (`mod watcher;`, command `dashboard_watch_repos`, command `dashboard_active_pipelines`, struct `ActivePipeline`, `tauri::Manager`/state setup)
- `src/lib/watcher.ts` (novo — `startWatcher`, `subscribeFsChange`)
- `src/lib/dashboard.ts` (interface `ActivePipeline` + `fetchActivePipelines`)
- `src/components/LivePipelineCard.tsx` (novo)
- `src/pages/Home.tsx` (section "Em execução" entre `AggregateOverview` e `Projetos`)
- `src/pages/ProjectDetail.tsx` (cards inline acima das tabs Specs/About)
- `src/pages/SpecDetail.tsx` (staleTime/refetchInterval reduzidos + freshness indicator)
- `src/components/layout/Topbar.tsx` (live indicator antes do kbd Ctrl+K)
- `src/App.tsx` (useEffects para `startWatcher` + `subscribeFsChange`)

Fora de boundary: schemas Tauri auto-gerados, hooks Mustard (`.claude/hooks/*`), demais comandos Tauri, Sidebar, Settings, Activity, Knowledge, Telemetry (essas não precisam de invalidate adicional — `events` kind já cobre `['telemetry', repoPath]` por prefix-match no tanstack-query), `mustard.json`, `query-client.ts` (já existe).

## Arquivos

Novos (~4):
- `src-tauri/src/watcher.rs`
- `src/lib/watcher.ts`
- `src/components/LivePipelineCard.tsx`

Modificados (~8):
- `src-tauri/Cargo.toml`
- `src-tauri/src/lib.rs`
- `src/lib/dashboard.ts`
- `src/pages/Home.tsx`
- `src/pages/ProjectDetail.tsx`
- `src/pages/SpecDetail.tsx`
- `src/components/layout/Topbar.tsx`
- `src/App.tsx`

## Component Contract

### `src/components/LivePipelineCard.tsx`

**Propósito:** Render denso (Linear-like) de uma pipeline ativa, reusado em Home, ProjectDetail e qualquer outra surface futura.

**Props:**
- `pipeline: ActivePipeline` (vem do `dashboard_active_pipelines`)
- `projectName?: string` — exibido como prefixo se passado (na Home agregada)
- `onClick?: () => void` — navegação opcional para SpecDetail

**Layout (compact, sem cards aninhados):**
1. **Linha primária:** `<StatusDot variant={phaseVariant} pulse={phase === 'EXECUTE'} />` + (se `projectName`) `<span className="text-muted-foreground text-[12px]">{projectName}</span>` + `<span className="font-mono text-[13px]">{spec_name}</span>` + `<Badge variant="outline" className="text-[10px] font-mono">{phase}</Badge>` + (se `model`) `<Badge variant="secondary" className="text-[10px]">{model}</Badge>` + `<span className="ml-auto text-[12px] text-muted-foreground">{updated_at ? relativeTime(updated_at) : '—'}</span>`
2. **Wave bar (se `current_wave` e `total_waves`):** `<div className="flex items-baseline gap-2 text-[12px] text-muted-foreground"><span>W{current_wave}/{total_waves}</span><div className="flex-1 h-1 bg-muted rounded overflow-hidden"><div className="h-full bg-amber-500/40" style={{ width: \`${(current_wave/total_waves)*100}%\` }} /></div></div>`
3. **Tasks progress (se algum task_* > 0):** `<div className="text-[12px] text-muted-foreground flex items-center gap-2"><span>{tasks_completed}/{tasks_pending+tasks_in_progress+tasks_completed} done</span><div className="flex h-1 w-32 rounded overflow-hidden bg-muted"><div className="bg-emerald-500/40" style={{ width: \`${completedPct}%\` }} /><div className="bg-amber-500/40" style={{ width: \`${inProgressPct}%\` }} /></div></div>`
4. **Failure banner (se `has_dispatch_failure`):** `<div className="text-[12px] rounded px-2 py-1 bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400">Dispatch failed {formatDurationMs(failure_age_ms ?? 0)} ago — run <code className="font-mono">/resume</code></div>`

**`phaseVariant` map (importar `StatusDotVariant` de `StatusDot.tsx`):** `ANALYZE|PLAN|QA → 'planning'`, `EXECUTE → 'active'`, `CLOSE → 'done'`. Se `has_dispatch_failure` → override para `'blocked'`.

**Container:** `<li className="flex flex-col gap-1 px-2 py-1.5 rounded hover:bg-muted/40 cursor-pointer">` (cursor só se `onClick` passado).

**Sem efeitos colaterais.** Wrapper puro de dados.

## Tarefas

### Backend Agent (Wave 1 — Rust watcher + active-pipelines)

- [ ] Em `src-tauri/Cargo.toml`, adicionar dependências `notify = "6"` (RecommendedWatcher) e `notify-debouncer-mini = "0.4"` (compatível com notify v6). Se cargo check apontar version mismatch, escolher versões compatíveis (a janela 6.x / 0.4.x é estável; documentar a escolha em comment se desviar)
- [ ] Criar `src-tauri/src/watcher.rs` com: `pub struct WatcherState { watchers: HashMap<String, notify_debouncer_mini::Debouncer<RecommendedWatcher>>, last_emit: HashMap<String, Instant> }` (Default derivado), helper `pub fn classify_kind(path: &Path) -> Option<&'static str>` que retorna `Some("events")` para paths contendo `".harness/events.jsonl"`, `Some("pipeline-state")` para paths contendo `".pipeline-states"`, `Some("spec")` para paths contendo `"spec/active"` ou `"spec\\active"` (Windows), `None` caso contrário. E `pub fn ensure_watching(state: Arc<Mutex<WatcherState>>, repo_path: String, app: AppHandle) -> Result<(), String>`: se já tem entry pra `repo_path` no `watchers`, retorna `Ok(())`. Caso contrário, cria debouncer com `Duration::from_millis(200)`, watch recursivo em `Path::new(&repo_path).join(".claude")`, no callback: para cada event/path, `classify_kind`, se `Some(kind)` emite `app.emit("dashboard:fs-change", FsChangePayload { repo_path: repo_path.clone(), kind: kind.to_string() })`. Throttle adicional (último emit por `(repo_path, kind)` <100ms → skip) usando `last_emit` no state. Logar via `eprintln!` se watch falhar (fail-soft)
- [ ] Em `src-tauri/src/lib.rs`: adicionar `mod watcher;` no topo. Em `pub fn run()`: `.manage(std::sync::Arc::new(std::sync::Mutex::new(watcher::WatcherState::default())))` no Builder antes do `.invoke_handler`. Adicionar `#[tauri::command] fn dashboard_watch_repos(repo_paths: Vec<String>, state: tauri::State<Arc<Mutex<WatcherState>>>, app: AppHandle) -> Result<(), String>` que itera `repo_paths` e chama `watcher::ensure_watching(state.inner().clone(), path, app.clone())` para cada. Registrar no `generate_handler![]`
- [ ] Em `src-tauri/src/lib.rs`: adicionar struct `pub struct ActivePipeline { spec_name: String, status: String, phase: String, current_wave: Option<u32>, total_waves: Option<u32>, model: Option<String>, has_dispatch_failure: bool, failure_age_ms: Option<u64>, tasks_pending: usize, tasks_in_progress: usize, tasks_completed: usize, updated_at: Option<String> }` com `#[derive(Serialize)] #[serde(rename_all = "snake_case")]`
- [ ] Em `src-tauri/src/lib.rs`: implementar `#[tauri::command] fn dashboard_active_pipelines(repo_path: String) -> Result<Vec<ActivePipeline>, String>`. Walk `<repo>/.claude/.pipeline-states/*.json` (skip arquivos que termine em `.metrics.json`). Para cada arquivo: parsear JSON tolerante (falha → skip). Extrair `specName || name || file_stem`, `status || "unknown"`, `phaseName || phase || "UNKNOWN"`, `currentWave/totalWaves/model` opcionais. Para `lastDispatchFailure`: se objeto presente, `has_dispatch_failure = true`, `failure_age_ms = (Utc::now() - parsed at)` em ms (skip se data não parseia). Para `tasks` array (se presente): contar por `status` ∈ `pending|in_progress|completed`. `updated_at = checkpointedAt || updatedAt || file mtime ISO` (fallback). **Filtrar:** excluir `status ∈ {"completed", "closed"}`. Ordenar desc por `updated_at`. Registrar em `generate_handler![]`
- [ ] Verificar compilação: `cargo check --manifest-path src-tauri/Cargo.toml` deve passar sem warnings novos

### Frontend Agent (Wave 2 — TS bridge + LivePipelineCard)

**Dependência:** Wave 1 completa (commands + event registrados via IPC).

- [ ] Criar `src/lib/watcher.ts` exportando: `export async function startWatcher(repoPaths: string[]): Promise<void>` que chama `invoke('dashboard_watch_repos', { repoPaths })`; `export function subscribeFsChange(): Promise<() => void>` que chama `listen<{ repo_path: string; kind: string }>('dashboard:fs-change', ...)` e mapeia: `kind === 'events'` → invalidate `['recent-events', repo_path]`, `['metrics', repo_path]`, `['activity']`, `['telemetry', repo_path]`; `kind === 'pipeline-state'` → invalidate `['active-pipelines', repo_path]`, `['specs', repo_path]`, `['pipelines', repo_path]`; `kind === 'spec'` → invalidate `['specs', repo_path]`, `['spec-md', repo_path]`. Importar `queryClient` de `./query-client` (já existente). Retorna a Promise do unlisten do Tauri
- [ ] Em `src/lib/dashboard.ts`, adicionar interface TS `ActivePipeline { spec_name: string; status: string; phase: string; current_wave: number | null; total_waves: number | null; model: string | null; has_dispatch_failure: boolean; failure_age_ms: number | null; tasks_pending: number; tasks_in_progress: number; tasks_completed: number; updated_at: string | null }` e função `export function fetchActivePipelines(repoPath: string): Promise<ActivePipeline[]> { return invoke<ActivePipeline[]>("dashboard_active_pipelines", { repoPath }); }`
- [ ] Criar `src/components/LivePipelineCard.tsx` conforme o Component Contract acima. Imports: `StatusDot, StatusDotVariant` de `@/components/StatusDot`, `Badge` de `@/components/ui/badge`, `relativeTime` de `@/lib/time`, `formatDurationMs` de `@/lib/format`, tipo `ActivePipeline` de `@/lib/dashboard`. Computar `total = tasks_pending + tasks_in_progress + tasks_completed`; `completedPct = total > 0 ? (tasks_completed/total)*100 : 0`; `inProgressPct = total > 0 ? (tasks_in_progress/total)*100 : 0`. Exportar como named `LivePipelineCard`

### Frontend Agent (Wave 3 — Integração nas páginas + watcher startup)

**Dependência:** Wave 2 completa (`LivePipelineCard` + `fetchActivePipelines` + `subscribeFsChange` disponíveis).

- [ ] Em `src/App.tsx`: adicionar dois `useEffect` no topo do componente, ambos dependentes de `useQuery(['discover', projectsRoot])` reusando a cache (queryKey idêntica à do Sidebar). Effect 1: quando `projects` muda, `startWatcher(projects.map(p => p.path))` (catch + console.error). Effect 2 (mount only, `[]` deps): `const p = subscribeFsChange(); return () => { p.then(u => u()).catch(() => {}); }`. Importar `useEffect` do react, `useQuery` do `@tanstack/react-query`, `discoverProjects` de `@/api/discovery`, `useStore` de `@/lib/store`, `startWatcher, subscribeFsChange` de `@/lib/watcher`
- [ ] Em `src/pages/Home.tsx`: adicionar entre `<AggregateOverview />` e o `<Separator />` (linha 49) uma nova section "Em execução". Usar `useQueries` do `@tanstack/react-query` para iterar `projects` e chamar `fetchActivePipelines(p.path)` por projeto, `staleTime: 5_000`, `refetchInterval: 12_000`. Agregar `const all = queries.flatMap(q => (q.data ?? []).map(pipeline => ({ pipeline, project: p }))).sort((a,b) => +new Date(b.pipeline.updated_at ?? 0) - +new Date(a.pipeline.updated_at ?? 0)).slice(0, 5)`. Renderizar `<ul className="flex flex-col gap-0.5">` de `<LivePipelineCard pipeline={...} projectName={...} onClick={() => navigate(\`/project/${project.id}/spec/${pipeline.spec_name}\`)} />`. Empty → `<p className="text-[13px] text-muted-foreground">Nenhuma pipeline em execução.</p>`. Heading: `<h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground">Em execução</h2>` (mesmo estilo do "Projetos")
- [ ] Em `src/pages/ProjectDetail.tsx`: na função do componente, adicionar `useQuery(['active-pipelines', project?.path], () => fetchActivePipelines(project!.path), { enabled: !!project, staleTime: 5_000, refetchInterval: 12_000 })`. Acima das tabs Specs/About (mas abaixo do header do projeto), se `data?.length > 0` renderizar `<ul className="flex flex-col gap-0.5 mb-4">` com até 3 `<LivePipelineCard>` (sem `projectName` — já estamos no contexto do projeto). Heading: `<h2 className="text-xs uppercase tracking-wider font-medium text-muted-foreground mb-1">Em execução</h2>`
- [ ] Em `src/pages/SpecDetail.tsx`: ajustar o `useQuery` que busca o markdown da spec — `staleTime: 10_000`, `refetchInterval: 30_000`. No header, exibir `<span className="text-[10px] text-muted-foreground">Atualizado {relativeTime(new Date(dataUpdatedAt).toISOString())}</span>` (usar `dataUpdatedAt` do retorno do `useQuery`). Renderizar apenas se `dataUpdatedAt > 0`
- [ ] Em `src/components/layout/Topbar.tsx`: importar `useQuery` (já tem useQueryClient), `discoverProjects`, `useQueries`, `fetchActivePipelines`. Computar `hasActive = projects?.some(p => (fetchedFor[p.path]?.length ?? 0) > 0)` — na prática usar `useQueries` agregando `active-pipelines` por projeto com `staleTime: 5_000`. Inserir entre `<nav>` (breadcrumb, linha 33) e a `<div>` direita (linha 34): `{hasActive && <div className="flex items-center gap-1 text-[10px] text-muted-foreground"><span className="w-1.5 h-1.5 rounded-full bg-emerald-500 animate-pulse" /><span>live</span></div>}`
- [ ] Verificar build: `pnpm run build` deve passar sem erros de tipo

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent. (Header kept in English because `qa-run.js` does not yet parse the PT heading "Critérios de Aceitação".)

- [x] AC-1: Build TypeScript passa — Command: `pnpm run build`
- [x] AC-2: cargo check passa sem warnings novos — Command: `node -e "const{execSync}=require('child_process');const path=require('path');const cargo=path.join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin',process.platform==='win32'?'cargo.exe':'cargo');execSync(JSON.stringify(cargo)+' check --manifest-path src-tauri/Cargo.toml --message-format=short',{stdio:'inherit'})"`
- [x] AC-3: notify dependency declarada em Cargo.toml — Command: `node -e "const c=require('fs').readFileSync('src-tauri/Cargo.toml','utf8');process.exit((c.includes('notify ')||c.includes('notify='))&&c.includes('notify-debouncer-mini')?0:1)"`
- [x] AC-4: watcher.rs declara WatcherState + classify_kind + ensure_watching — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/watcher.rs','utf8');process.exit((c.includes('WatcherState')&&c.includes('classify_kind')&&c.includes('ensure_watching'))?0:1)"`
- [x] AC-5: dashboard_watch_repos e dashboard_active_pipelines registrados no invoke_handler — Command: `node -e "const c=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const occW=c.split('dashboard_watch_repos').length-1;const occA=c.split('dashboard_active_pipelines').length-1;process.exit((c.includes('generate_handler')&&occW>=2&&occA>=2)?0:1)"`
- [x] AC-6: watcher.ts exporta startWatcher e subscribeFsChange — Command: `node -e "const c=require('fs').readFileSync('src/lib/watcher.ts','utf8');process.exit((c.includes('export async function startWatcher')&&c.includes('export function subscribeFsChange'))?0:1)"`
- [x] AC-7: LivePipelineCard existe e Home/ProjectDetail/Topbar consumem o pipeline-status — Command: `node -e "const fs=require('fs');const card=fs.existsSync('src/components/LivePipelineCard.tsx');const home=fs.readFileSync('src/pages/Home.tsx','utf8').includes('LivePipelineCard');const top=fs.readFileSync('src/components/layout/Topbar.tsx','utf8').includes('live');process.exit((card&&home&&top)?0:1)"`
- [x] AC-8: App.tsx inicializa watcher + listener fs-change — Command: `node -e "const c=require('fs').readFileSync('src/App.tsx','utf8');process.exit((c.includes('startWatcher')&&c.includes('subscribeFsChange'))?0:1)"`

## Preocupações

- WARN (analyze-validation, layer-gap, x3): heurística não detectou extensões `.rs` / `.tsx` nas seções Backend Agent / Frontend Agent. Falso positivo conhecido — `src-tauri/Cargo.toml`, `src-tauri/src/watcher.rs`, `src-tauri/src/lib.rs`, `src/lib/watcher.ts`, `src/lib/dashboard.ts`, `src/components/LivePipelineCard.tsx`, `src/pages/Home.tsx`, `src/pages/ProjectDetail.tsx`, `src/pages/SpecDetail.tsx`, `src/components/layout/Topbar.tsx`, `src/App.tsx` estão listados em `## Arquivos`. Sem ação.

## Não-Objetivos

- Não persistir dispatch failure state na UI além do que vem do pipeline-state JSON — não criar um store de erros
- Não criar componentes para "PRD builder" / "Commands catalog" / "ENV editor" / "Glossary" / "license" — mencionados pelo usuário como Wave D/E/F futuros, fora deste escopo
- Não migrar polling residual (SQLite-based queries continuam com seus `staleTime` atuais) — watcher complementa, não substitui
- Não adicionar SSE / WebSocket / HTTP server local — Tauri event system é suficiente
- Não tocar em `query-client.ts` (já existente) nem refatorar o setup do `QueryClient` em `main.tsx`
- Não escutar mudanças em arquivos fora de `.claude/` (knowledge.json, mustard.json, etc são raros e não justificam o ruído)
- Não criar uma view dedicada `/pipelines` ou rota nova — `LivePipelineCard` é incrustado nas surfaces existentes
- Não adicionar ícones animados de loading dentro do card — `StatusDot` com `pulse` já comunica atividade
- Não adicionar lógica de retry/auto-resume — apenas mostrar `has_dispatch_failure` + sugerir `/resume` no banner; ação manual do usuário
