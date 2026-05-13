# Feature: dashboard-project-detail

### Status: closed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-12T00:00:00Z
### Lang: pt

## Contexto

A Home da Mustard Dashboard, depois de `dashboard-home-real`, mostra três contagens vivas (pipelines, métricas, knowledge) — mas a aplicação ainda é uma tela única: não há rota, não há navegação, e o "projeto" Mustard como entidade integral (subprojects detectados, recipes registradas, skills disponíveis, eventos recentes) não tem onde aparecer. Esses dados existem no scaffold (`sync-detect.js`, `.claude/recipes/*.json`, `.claude/skills/*/SKILL.md`, `.claude/commands/mustard/*/SKILL.md`, `.claude/.harness/events.jsonl`) mas só são acessíveis via terminal. O impacto observável é que, ao abrir o app, o usuário vê os totais agregados mas não consegue inspecionar o que de fato o orquestrador tem em mãos — quais subprojetos foram detectados, que recipes estão prontas, ou o que o harness vem registrando neste momento. Sem uma tela dedicada o dashboard fica parado no nível de "indicadores", longe de servir como mission control real.

## Summary

Introduzir roteamento (`HashRouter` de React Router 7 — canonical em Tauri 2 desktop) e uma rota `/project` com a tela `ProjectDetail` que agrega quatro novos Tauri commands: subprojects detectados, recipes catalogadas, skills disponíveis e eventos recentes do harness. Sidebar passa a usar `NavLink` ativando estado visual por rota.

## Entity Info

N/A — registry vazio. Os DTOs novos (`SubprojectInfo`, `RecipeMeta`, `SkillMeta`, `RecentEvent`) são structs de leitura locais ao dashboard, espelhados em `src/lib/dashboard.ts`. "Project" aqui é um agregador conceitual, não uma entidade persistida.

## Boundaries

Caminhos intencionalmente tocados:
- `src-tauri/src/lib.rs`
- `src/lib/dashboard.ts`
- `src/hooks/useProject.ts` (novo)
- `src/pages/ProjectDetail.tsx` (novo)
- `src/App.tsx`
- `src/components/layout/Sidebar.tsx`
- `package.json`

Fora de escopo (qualquer edit aqui surface `[BOUNDARY WARNING]`):
- `src/pages/Home.tsx` (continua funcional sem alteração)
- `src/components/layout/AppShell.tsx` / `Topbar.tsx`
- `src/components/ui/*`
- `src-tauri/Cargo.toml` (sem deps novas; `std::process::Command` é stdlib)
- `src-tauri/capabilities/default.json`
- Loader/Error boundaries do React Router (uso declarativo `<Routes>`/`<Route>`, sem data routers/loaders/actions)

## Files (~7)

| Path | Operação |
|------|----------|
| `package.json` | modify (adicionar `react-router` ^7) |
| `src-tauri/src/lib.rs` | modify (adicionar 4 commands + structs serde + registrar em `invoke_handler!`) |
| `src/lib/dashboard.ts` | modify (adicionar 4 tipos + 4 fetchers) |
| `src/hooks/useProject.ts` | create (hook agregador com `Promise.all` dos 4 fetchers) |
| `src/pages/ProjectDetail.tsx` | create (renderiza 4 seções: subprojects, recipes, skills, eventos recentes) |
| `src/App.tsx` | modify (wrap em `HashRouter`, definir `<Routes>` com `/` e `/project`) |
| `src/components/layout/Sidebar.tsx` | modify (trocar `<a href>` por `<NavLink>`, adicionar item "Projeto") |

## Component Contract

`ProjectDetail` é a única tela nova; sem props.

| Prop | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| — | — | — | — | Sem props (componente de rota) |

Visual states (página inteira sob o `<main>` do `AppShell`, padding já herdado):
- **loading** — header `<h1>Projeto Mustard</h1>` + 4 sub-blocos cada um com `<p className="text-muted-foreground">Carregando…</p>`.
- **error** — header + `<p className="text-destructive">{error}</p>`. Sem fallback parcial; o hook agrega todas as falhas em uma `string`.
- **data** — 4 sub-blocos sequenciais:
  - **Subprojects:** `<h2>Subprojects ({n})</h2>` + se `n=0` `<p>Nenhum subprojeto detectado.</p>`; senão lista `<ul>` de `name (role)`.
  - **Recipes:** `<h2>Recipes ({n})</h2>` + lista `<ul>` `name — description` (description truncada a 120 chars).
  - **Skills:** `<h2>Skills ({n})</h2>` + lista `<ul>` `name — description` (idem trunc).
  - **Eventos recentes:** `<h2>Eventos recentes (últimos {n})</h2>` + lista `<ul>` `type @ ts` mais legível; máximo 20 itens.

Sidebar: dois itens (`Home` → `/`, `Projeto` → `/project`) usando `<NavLink>`. Estado ativo aplica `bg-muted text-foreground font-medium`; inativo `text-sidebar-foreground/80 hover:bg-muted/60`.

Sem variantes, sem props públicas; usa Tailwind direto (sem `Card` para a página inteira — Card só faz sentido para tiles, e aqui temos seções de texto/listas).

## Tasks

### Backend Agent (Wave 1)

- [x] Em `src-tauri/src/lib.rs`, definir 4 structs serde `Serialize`:
  - `SubprojectInfo { name: String, role: Option<String> }`
  - `RecipeMeta { name: String, description: String }`
  - `SkillMeta { name: String, description: String, source: String }` (source = "foundation" | "command")
  - `RecentEvent { event_type: String, ts: Option<String>, summary: Option<String> }`
- [x] `#[tauri::command] fn dashboard_subprojects() -> Result<Vec<SubprojectInfo>, String>`: executa `std::process::Command::new("node").arg(".claude/scripts/sync-detect.js")` com `cwd = current_dir()`. Parse stdout como JSON; extrai `subprojects[]` e `detectedAgents[]` para popular `role`. Node ausente / script falha → `Err("...")`. Output vazio (`subprojects: []`) → `Ok(vec![])`.
- [x] `#[tauri::command] fn dashboard_recipes() -> Result<Vec<RecipeMeta>, String>`: enumera `.claude/recipes/*.json`, parse cada um, extrai `{ name, description }`. Skip arquivos malformados (log e continue). Diretório ausente → `Ok(vec![])`.
- [x] `#[tauri::command] fn dashboard_skills() -> Result<Vec<SkillMeta>, String>`: enumera `.claude/skills/*/SKILL.md` (source="foundation") e `.claude/commands/mustard/*/SKILL.md` (source="command"). Extrai `name` e `description` do frontmatter YAML (parser simples: linha a linha entre `---` e `---`, formato `chave: valor`). Sem dependência de crate YAML — parse manual. Diretórios ausentes → `Ok(vec![])`.
- [x] `#[tauri::command] fn dashboard_recent_events(limit: Option<usize>) -> Result<Vec<RecentEvent>, String>`: tail de `.claude/.harness/events.jsonl` pegando últimas `limit.unwrap_or(20)` linhas válidas; parse cada uma; popula `event_type` do campo `type`, `ts` do campo `ts`/`timestamp`, `summary` do campo `summary`/`description` se existir. Arquivo ausente → `Ok(vec![])`.
- [x] Atualizar `invoke_handler!` adicionando os 4 novos commands (manter os 3 anteriores: `dashboard_pipelines`, `dashboard_metrics`, `dashboard_knowledge`).
- [x] Validar: `cargo check --manifest-path src-tauri/Cargo.toml`.

### Frontend Agent (Wave 2)

- [x] `package.json`: adicionar `"react-router": "^7"` em `dependencies`. Sem `-dom` suffix (consolidado em v7). Rodar `pnpm install` ao final do agent.
- [x] `src/lib/dashboard.ts`: adicionar tipos `SubprojectInfo`, `RecipeMeta`, `SkillMeta`, `RecentEvent` (espelhar Rust snake_case) e funções `fetchSubprojects()`, `fetchRecipes()`, `fetchSkills()`, `fetchRecentEvents(limit?: number)`. Manter as 3 funções/tipos anteriores intactas.
- [x] `src/hooks/useProject.ts` (novo): hook idiomático que faz `Promise.all` dos 4 fetchers (passando `limit=20` para `fetchRecentEvents`) no `useEffect` inicial. Expõe `{ subprojects, recipes, skills, recentEvents, loading, error }`. Sem polling.
- [x] `src/pages/ProjectDetail.tsx` (novo): export named `ProjectDetail`, consome `useProject()`, renderiza header + 4 seções conforme Component Contract.
- [x] `src/App.tsx`: importar `HashRouter`, `Routes`, `Route` de `react-router`; envolver `AppShell` numa `HashRouter` raiz; trocar `<Home />` direto por `<Routes>` com duas rotas (`/` → `Home`, `/project` → `ProjectDetail`). Importar `ProjectDetail`.
- [x] `src/components/layout/Sidebar.tsx`: importar `NavLink` de `react-router`; substituir o `<a href="/">` por `<NavLink to="/">` com `className` baseada em `isActive` (`bg-muted text-foreground font-medium` ativo; padrão inativo). Adicionar segundo item `<NavLink to="/project">Projeto</NavLink>`.
- [x] Validar: `pnpm tsc --noEmit`.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript compila — Command: `pnpm tsc --noEmit`
- [x] AC-2: Rust compila — Command: `node -e "const{execSync}=require('child_process');const path=require('path');const candidates=['cargo',path.join(process.env.USERPROFILE||process.env.HOME||'','.cargo','bin','cargo.exe')];const bin=candidates.find(c=>{try{execSync((c==='cargo'?c:JSON.stringify(c))+' --version',{stdio:'pipe'});return true}catch{return false}});if(!bin){console.error('cargo not found');process.exit(1)}execSync((bin==='cargo'?bin:JSON.stringify(bin))+' check --manifest-path src-tauri/Cargo.toml',{stdio:'inherit'})"`
- [x] AC-3: `react-router` está em `dependencies` — Command: `node -e "const p=require('./package.json');process.exit(p.dependencies && p.dependencies['react-router']?0:1)"`
- [x] AC-4: `App.tsx` usa `HashRouter` + `Routes` — Command: `node -e "const s=require('fs').readFileSync('src/App.tsx','utf8');process.exit(s.includes('HashRouter')&&s.includes('Routes')&&s.includes('/project')?0:1)"`
- [x] AC-5: `ProjectDetail.tsx` existe e exporta `ProjectDetail` — Command: `node -e "const s=require('fs').readFileSync('src/pages/ProjectDetail.tsx','utf8');process.exit(/export\s+(function|const)\s+ProjectDetail/.test(s)?0:1)"`
- [x] AC-6: `lib.rs` expõe os 4 novos commands + mantém os 3 anteriores — Command: `node -e "const s=require('fs').readFileSync('src-tauri/src/lib.rs','utf8');const all=['dashboard_pipelines','dashboard_metrics','dashboard_knowledge','dashboard_subprojects','dashboard_recipes','dashboard_skills','dashboard_recent_events'];process.exit(all.every(c=>s.includes('fn '+c))?0:1)"`
- [x] AC-7: Sidebar usa `NavLink` para `/project` — Command: `node -e "const s=require('fs').readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit(s.includes('NavLink')&&s.includes('/project')?0:1)"`

## Concerns

- **HashRouter vs BrowserRouter:** escolhido HashRouter porque em produção Tauri 2 o app é servido via protocolo customizado (`tauri://localhost`) onde `pushState` pode interagir mal com asset URLs. HashRouter mantém deep-link via `#/path` sem servidor, é o padrão recomendado para SPAs desktop (Tauri/Electron). Trade-off conhecido: URLs com `#/` são menos elegantes — aceitável para app desktop interno.
- **Rust → Node bridge:** `dashboard_subprojects()` executa `node .claude/scripts/sync-detect.js` via `Command::new`. Acopla o Tauri ao Node estar no PATH do processo do usuário. É aceitável porque o Mustard scaffold já depende de Node para tudo (pnpm, scripts, hooks); se Node sumir, o app inteiro está quebrado. Alternativa rejeitada: replicar a lógica de `sync-detect` em Rust (duplicação + risco de drift).
- **Parser YAML manual:** `dashboard_skills()` extrai `name`/`description` do frontmatter via parse linha a linha (regex simples ou split em `:`). Não introduzimos `serde_yaml` para evitar peso de dep. Limitação: não suporta valores multi-line ou aspas escapadas. Caso o frontmatter dos SKILL.md vire mais elaborado, vira spec separada para adotar `serde_yaml`.
- **Sem testes E2E:** scaffold ainda não tem Playwright/Vitest configurado. Validação fica em build + tsc + cargo check + verificações estruturais. Smoke visual continua sendo via `pnpm tauri:dev` manual.
- [VALIDATOR] WARN missing-file `src/hooks/useProject.ts` — marcado como `create` na tabela Files (falso positivo do parser).
- [VALIDATOR] WARN missing-file `src/pages/ProjectDetail.tsx` — idem.
- [VALIDATOR] WARN missing-file `Promise.all` — falso positivo (regex do validator capturou `Promise.all` no texto da Tasks).

## Non-Goals

- Loaders / actions / data routers do React Router v7 (uso declarativo simples basta)
- Deep-link individual a um item (ex: `/project/recipes/add-field`) — para um próximo spec
- Filtros, busca ou ordenação nas listas
- Refresh manual / auto-refresh / file watching
- Persistência de path raiz (cwd-relative permanece, conforme [[project-dashboard-design-intent]])
- Componentização avançada das seções (sem `<Section>` reutilizável; inline na primeira iteração)
- Setup de ESLint, Playwright, Vitest
- Tratamento da pasta duplicada `src-tauri/src-tauri/`

## Dependencies

Adiciona `react-router` ^7 às `dependencies` do `package.json` (pacote consolidado; sem `react-router-dom` separado em v7). Rust não ganha deps novas — `std::process::Command` + `serde`/`serde_json` (já presentes) cobrem todo o trabalho.
