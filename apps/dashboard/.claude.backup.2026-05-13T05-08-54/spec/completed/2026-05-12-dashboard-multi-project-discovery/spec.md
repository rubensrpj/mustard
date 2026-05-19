# Feature: dashboard-multi-project-discovery

### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-12T23:00:00Z
### Lang: pt

## Contexto

O Mustard Dashboard nasceu para ser app standalone capaz de inspecionar múltiplos projetos Mustard descobertos no filesystem do usuário, exibindo specs, métricas e knowledge de cada um separadamente — é a tese central registrada em REFERENCE.md ("visão multi-projeto consolidada" como valor pago). Hoje o app está parcialmente implementado: o backend Rust descobre dados via `find_mustard_root()` que escala caminhos a partir do CWD do próprio processo, e a UI usa rota `/project` sem parâmetro mostrando um único projeto. O resultado é que o dashboard é self-dogfooding e não consegue inspecionar nenhum outro projeto além do próprio repo `mustard-dashboard`. O impacto é direto: até esta wave o produto não tem como demonstrar seu diferencial — qualquer usuário que rode o app só vê o próprio repo, e não a sua coleção real de projetos Mustard.

## Limites

Esta wave toca exclusivamente os caminhos enumerados em `## Arquivos`. Edições fora dessa lista devem disparar `[BOUNDARY WARNING]`. Específicamente fora de escopo: SpecDetail drill-down, AggregateView, KnowledgeBrowser, license gate, CI cross-platform, auto-updater, filesystem watcher.

## Resumo

Adicionar discovery filesystem (Rust BFS depth 5 a partir de root configurável), store global persistente (Zustand + localStorage), página Settings para configurar root, refatorar Home para listar projetos descobertos com status dots, parametrizar ProjectDetail por id, propagar `repo_path` para todos os 7 commands Rust (que hoje usam CWD), e adicionar action "Switch to <project>" no Cmd+K.

## Entity Info

**Entity:** `Project` (nova — entity-registry.json vazio, esperado até `/scan` rodar pós-merge).

Shape (contrato Rust ↔ TS):

```rust
struct Project {
  id: String,                      // FNV-1a hex 16 chars do path canônico
  name: String,                    // basename do path
  path: String,                    // dir absoluto contendo .claude/
  db_path: String,                 // path/.claude/.harness/mustard.db
  last_activity_ms: Option<u64>,   // max mtime de events.jsonl ou mustard.db
}
```

## Arquivos (~14)

**Rust (4):**
- `src-tauri/src/discovery.rs` (NOVO)
- `src-tauri/src/lib.rs` (mod discovery + rewrite 7 commands para aceitar `repo_path`)
- `src-tauri/Cargo.toml` (adicionar `tauri-plugin-dialog`)
- `src-tauri/capabilities/default.json` (permissões dialog:default)

**Frontend (10):**
- `package.json` (`zustand`, `@tanstack/react-query`, `@tauri-apps/plugin-dialog`)
- `src/lib/store.ts` (NOVO — Zustand persist)
- `src/api/discovery.ts` (NOVO — invoke bridge + type Project)
- `src/lib/query-client.ts` (NOVO — QueryClient singleton)
- `src/main.tsx` (wrap com QueryClientProvider)
- `src/App.tsx` (rota `/settings` e `/project/:id`)
- `src/pages/Settings.tsx` (NOVO)
- `src/pages/Home.tsx` (substituir card Pipelines por seção Projects)
- `src/pages/ProjectDetail.tsx` (useParams + breadcrumb + project lookup)
- `src/hooks/useProject.ts` (aceitar `project: Project` e passar paths)
- `src/components/CommandPalette.tsx` (Switch to <project> actions)

## Component Contract

**`Settings.tsx`** (nova page)
- **Purpose**: configurar `projectsRoot` global e previewar projetos descobertos.
- **Props**: nenhuma (lê store global).
- **States**: `idle` (sem root), `discovering` (loading), `populated` (lista N projetos com `relativeTime(last_activity_ms)`), `error` (root inválido / zero projetos).
- **Acessibilidade**: botão "Selecionar pasta" focável via Tab; dialog Tauri tem foco nativo.
- **Tokens**: `surface`, `muted-foreground` existentes — sem novas vars CSS.

**Projects section (em `Home.tsx`)**
- **Purpose**: listar projetos descobertos com status dot + last_activity relativa.
- **States**: `empty` (projectsRoot null → CTA "Configure em Settings →"), `loading` (discoverProjects pending), `populated` (lista densa), `error` (root walkable mas zero projetos).
- **Variants**: linha densa default; `hover:bg-muted/40` consistente com Recipes/Skills lists do ProjectDetail.

## Plano

### Backend Agent (Wave 1)

- [ ] **Adicionar `tauri-plugin-dialog`** em `src-tauri/Cargo.toml` (`tauri-plugin-dialog = "2"`) e registrar `.plugin(tauri_plugin_dialog::init())` em `lib.rs#run()`. Atualizar `src-tauri/capabilities/default.json` permissions incluindo `dialog:default`.
- [ ] **Criar `src-tauri/src/discovery.rs`** com:
  - `pub struct Project { id, name, path, db_path, last_activity_ms }` derivando `Serialize`.
  - `pub fn discover(root: &Path) -> Result<Vec<Project>, String>` — BFS depth 5 via fila `VecDeque<(PathBuf, u32)>`.
  - Skip set (`HashSet<&str>`): `node_modules .git dist target .next vendor .obsidian src-tauri/target` (último checado contra `path.ends_with`).
  - Detection: dir D contém `D/.claude/.harness/mustard.db` → push `Project`.
  - `id` = FNV-1a 64-bit hex inline (sem dep): `let mut h:u64=0xcbf29ce484222325;for b in path.as_bytes(){h^=*b as u64;h=h.wrapping_mul(0x100000001b3);}`.
  - `last_activity_ms` = max de mtime (`SystemTime::duration_since(UNIX_EPOCH).as_millis()`) de `events.jsonl` e `mustard.db`; `None` se nenhum existir.
- [ ] **Registrar `discover_projects` tauri command** em `lib.rs`: `#[tauri::command] fn discover_projects(root: String) -> Result<Vec<discovery::Project>, String>`. Adicionar `mod discovery;` no topo. Incluir em `invoke_handler![]`.
- [ ] **Rewrite 7 commands para aceitar `repo_path: String`** em vez de `find_mustard_root()`: `dashboard_pipelines`, `dashboard_metrics`, `dashboard_knowledge`, `dashboard_subprojects`, `dashboard_recipes`, `dashboard_skills`, `dashboard_recent_events`. Para cada um trocar `let base = find_mustard_root()?;` por `let base = PathBuf::from(&repo_path);`. `dashboard_subprojects`: `current_dir(&base)` (já era). Manter `find_mustard_root` privada apenas se ainda houver caller — caso contrário deletar.
- [ ] **`cargo check --manifest-path src-tauri/Cargo.toml`** passa sem warnings novos.

### Frontend Agent (Wave 2)

- [ ] **Adicionar deps em `package.json`**: `zustand@^5`, `@tanstack/react-query@^5`, `@tauri-apps/plugin-dialog@^2`. Rodar `pnpm install`.
- [ ] **Criar store global + API bridge** (`src/lib/store.ts`, `src/api/discovery.ts`):
  - `store.ts`: Zustand + `persist` middleware (name `mustard-dashboard-store`) expondo `projectsRoot: string | null`, `selectedProjectId: string | null`, `setProjectsRoot`, `setSelectedProjectId`.
  - `discovery.ts`: type `Project` snake_case (consistente com commands existentes) + `discoverProjects(root: string): Promise<Project[]>` via `invoke('discover_projects', { root })`.
- [ ] **Setup TanStack Query Provider** (`src/lib/query-client.ts`, `src/main.tsx`): `queryClient = new QueryClient({ defaultOptions: { queries: { staleTime: 60_000, refetchOnWindowFocus: false } } })`. Em `main.tsx`, wrap `<App />` com `<QueryClientProvider client={queryClient}>`.
- [ ] **Atualizar `src/App.tsx`**: trocar `<Route path="/project" ...>` por `<Route path="/project/:id" ...>`. Adicionar `<Route path="/settings" element={<Settings />} />`.
- [ ] **Criar `src/pages/Settings.tsx`** (parallel-safe): card "Diretório de projetos" mostrando `projectsRoot` (placeholder "Não configurado" se null). Botão "Selecionar pasta" → `const selected = await open({ directory: true, multiple: false })` do `@tauri-apps/plugin-dialog` → `setProjectsRoot(typeof selected === 'string' ? selected : null)`. Abaixo: lista preview via `useQuery({ queryKey: ['discover', projectsRoot], queryFn: () => discoverProjects(projectsRoot!), enabled: !!projectsRoot, staleTime: 60_000 })` exibindo `name` + `relativeTime(last_activity_ms)`.
- [ ] **Refatorar `src/pages/Home.tsx`**: substituir card "Pipelines" por seção `## Projects` (mantém Métricas + Knowledge cards mas com `useDashboard(selectedProject)` — passam vazio se sem projeto). Empty state: "Configure o diretório de projetos em Settings →" com `<Link to="/settings">`. Populado: `useQuery(['discover', projectsRoot], ...)` com `enabled: !!projectsRoot, staleTime: 60_000`; cada item linha densa com `<StatusDot variant={p.last_activity_ms && Date.now()-p.last_activity_ms < 3_600_000 ? 'active' : 'idle'}>` + nome + `relativeTime(last_activity_ms)`. Click handler: `setSelectedProjectId(p.id); navigate('/project/' + p.id)`.
- [ ] **Refatorar `src/pages/ProjectDetail.tsx`**: `const { id } = useParams<{ id: string }>()`. Lookup: `const projects = queryClient.getQueryData<Project[]>(['discover', projectsRoot])` → `const project = projects?.find(p => p.id === id) ?? null`. Se `project === null` → empty state "Projeto não encontrado — volte ao [Home](/) ou configure root em [Settings](/settings)". Passar `project` para `useProject(project)`. Header: `<h1>{project.name}</h1>` + breadcrumb `Mustard / Projetos / {project.name}` acima do `<SectionHeading>` Subprojects. Effect: `useEffect(() => { if (id && id !== selectedProjectId) setSelectedProjectId(id); }, [id])` para sincronizar refresh.
- [ ] **Atualizar hooks de fetch** (`src/hooks/useProject.ts`, `src/hooks/useDashboard.ts`): ambos aceitam `project: Project | null`. `useProject` passa `{ repoPath: project.path }` aos 4 invokes (`dashboard_subprojects`, `dashboard_recipes`, `dashboard_skills`, `dashboard_recent_events`). `useDashboard` análogo para os 3 (`dashboard_pipelines`, `dashboard_metrics`, `dashboard_knowledge`). Se `project === null` → retornar shape vazia + `error: 'Sem projeto selecionado'`.
- [ ] **Atualizar `src/components/CommandPalette.tsx`**: ler `projects = queryClient.getQueryData<Project[]>(['discover', projectsRoot]) ?? []`. Para cada `p`, render `<Command.Item value="switch-{p.id}" onSelect={() => { setSelectedProjectId(p.id); navigate('/project/' + p.id); setOpen(false); }}>Switch to {p.name}</Command.Item>`. Manter ações existentes intactas. Cmdk filter já cobre fuzzy via `value` prop.
- [ ] **`pnpm build`** passa (tsc -b && vite build).

### Dependências entre Waves

Wave 1 (Backend) entrega contrato `discover_projects` + 7 commands com `repo_path`. Wave 2 (Frontend) consome esse contrato — **não parallel-safe** porque shape do invoke depende do Rust compilar primeiro. Recipes: nenhuma estruturada (registry vazio); usar `react-best-practices` + `karpathy-guidelines` skills.

## Preocupações

- **N+1 fetches por projeto**: status dot rico por projeto exigiria buscar `pipeline-states/*.json` de cada. Solução desta wave: derivar status apenas de `last_activity_ms < 1h → active` (proxy simples, evita N invokes paralelos). Status mais rico fica para spec futura.
- **`id` estável**: FNV-1a 64-bit é determinístico entre runs/versões (algoritmo fixo) e tem boa distribuição para paths. Sem dep externa.
- **`tauri-plugin-dialog` capabilities**: Tauri v2 exige `dialog:default` em `capabilities/default.json`; omitir = dialog falha silenciosamente em build release.
- **HashRouter + persist redundância**: URL é fonte da verdade para `selectedProjectId`; store é cache hot path. Effect em ProjectDetail sincroniza store quando `params.id` muda (refresh preserva via URL).
- **camelCase vs snake_case no contrato Rust→TS**: commands atuais retornam snake_case (`event_type`, `last_event_at`). Discovery.rs deve seguir mesmo padrão (`last_activity_ms`, `db_path`). TS interface `Project` usa snake_case para zero-friction (sem rename serde).
- **Tauri v2 dialog API**: `open({ directory: true })` retorna `string | null` (não array) quando `multiple: false`. Tipo verificar.
- **DefaultIgnore em discovery**: `src-tauri/target` matcha por `ends_with("src-tauri/target")` apenas se path contém esse sufixo; mais seguro skip qualquer `target` em qualquer depth (matchar `file_name() == "target"`).

## Não-Objetivos

- SpecDetail drill-down (clicar numa spec → nova view)
- AggregateView (métricas consolidadas de N projetos)
- KnowledgeBrowser
- License gate
- CI cross-platform
- Auto-updater wiring
- Filesystem watcher (re-discover só on-demand)
- Status dot rico por projeto (PLAN/EXECUTE aware) — usar `last_activity_ms` como proxy

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Frontend compila — Command: `pnpm build`
- [x] AC-2: Rust compila — Command: `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] AC-3: `discover_projects` + `mod discovery` registrados em lib.rs — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src-tauri/src/lib.rs','utf8');if(!c.includes('mod discovery')||!c.includes('discover_projects')){process.exit(1)}"`
- [x] AC-4: discovery.rs tem skip patterns + detecção mustard.db — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src-tauri/src/discovery.rs','utf8');for(const k of ['node_modules','.git','target','mustard.db']){if(!c.includes(k)){console.error('missing',k);process.exit(1)}}"`
- [x] AC-5: Store global expõe campos esperados — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/lib/store.ts','utf8');for(const k of ['projectsRoot','selectedProjectId','setProjectsRoot','setSelectedProjectId','persist']){if(!c.includes(k)){console.error('missing',k);process.exit(1)}}"`
- [x] AC-6: Rotas `/settings` e `/project/:id` presentes em App.tsx — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/App.tsx','utf8');if(!c.includes('/settings')||!c.includes('/project/:id')){process.exit(1)}"`
- [x] AC-7: Settings.tsx usa plugin-dialog em modo directory — Command: `node -e "const fs=require('fs');const c=fs.readFileSync('src/pages/Settings.tsx','utf8');if(!c.includes('@tauri-apps/plugin-dialog')||!c.includes('directory')){process.exit(1)}"`
- [x] AC-8: Plugin-dialog wired em Cargo.toml + capabilities — Command: `node -e "const fs=require('fs');const cargo=fs.readFileSync('src-tauri/Cargo.toml','utf8');const caps=fs.readFileSync('src-tauri/capabilities/default.json','utf8');if(!cargo.includes('tauri-plugin-dialog')){console.error('missing in Cargo');process.exit(1)}if(!caps.includes('dialog')){console.error('missing in capabilities');process.exit(1)}"`
