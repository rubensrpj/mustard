# Feature: dashboard-sidebar-tree

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: full
### Checkpoint: 2026-05-19T23:30:00Z
### Lang: pt

> Continuação de `b6-dashboard-projects`. A pipeline anterior entregou o registry de projetos no dashboard e a bridge `addProject → activeWorkspaceId`. Plano detalhado de referência: `.claude/plans/quase-ok-ajustes-1-prancy-cerf.md`.

## PRD

## Contexto

O dashboard hoje renderiza a sidebar em formato plano: a primeira linha é o `WorkspaceSwitcher` (dropdown para trocar de projeto ativo), seguido por uma entrada `Projects` (lista global) e cinco itens de "Workspace" (`Home/Activity/Telemetry/Quality/Knowledge`) que leem do projeto ativo, mais ferramentas globais (`Commands/Prd`) e um link para `Settings`. Esse layout mistura escopo: "Projects" é meta-conceito (gerencia a lista), enquanto os cinco itens abaixo são páginas-internas-do-projeto-ativo. O user mantém modelo mental por projeto, mas a UI força ele a saltar pelo switcher pra trocar contexto.

O modelo desejado é o de cliente de e-mail multi-conta: cada projeto vira um grupo expansível na sidebar (uma "conta"), e dentro dele aparecem as mesmas folhas (`Home/Activity/Telemetry/Quality/Knowledge/Settings`, análogo a `Inbox/Sent/Drafts`). Click numa folha = ativa o workspace + navega num único gesto, sem switcher separado. O dashboard deixa de ter "Settings global" — Settings vira folha por-projeto. O único setting hoje dashboard-global (toggle de idioma PT/EN) migra pra uma nova página `/preferences` acessível por um gear no rodapé da sidebar. Adicionar/remover/atualizar/desinstalar Mustard de um projeto vira ação contextual da raiz do nó na árvore (kebab `⋮`).

## Usuários/Stakeholders

Usuário do dashboard mustard que mantém múltiplos projetos paralelos. Pedido feito por @rubens em 2026-05-19 logo após o close de b6-dashboard-projects, depois de notar a tensão de design da sidebar plana ("e se o sidebar fosse um treeview o usuário adiciona as pastas, funcionar igual a uma conta de email onde cada grupo fosse uma conta?").

## Métrica de sucesso

Trocar de projeto exige no máximo 2 clicks (expandir nó + escolher folha), e adicionar um novo projeto não passa por nenhuma página intermediária (`+ Adicionar` no topo da árvore abre file picker direto).

## Não-Objetivos

- Não persistir o estado expand/collapse entre reloads do dashboard (in-memory por enquanto).
- Não suportar drag-reorder de projetos na árvore.
- Não criar sub-grupos dentro de um projeto (ex.: Monitoring/Reference como sub-grupos das folhas).
- Não adicionar tema do dashboard à Preferences agora (futuro).
- Não criar comando `mustard-cli uninstall` (CLI binary) — Tauri faz `fs::remove_*` direto.
- Não refatorar `/project/:id` e `/project/:id/spec/:specName` para path-as-id (legado, mantém como está).
- Não reescrever `Home/Activity/Telemetry/Quality/Knowledge/Prd/Commands` — eles seguem lendo `activeWorkspaceId`, sem refactor.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Dashboard builda após a refatoração — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: O Tauri backend compila com o novo `uninstall_mustard` — Command: `cargo check --manifest-path apps/dashboard/src-tauri/Cargo.toml`
- [x] AC-3: O comando Tauri `uninstall_mustard` está registrado — Command: `node -e "process.exit(require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8').includes('uninstall_mustard') ? 0 : 1)"`
- [x] AC-4: `WorkspaceSwitcher` foi deletado — Command: `node -e "process.exit(require('fs').existsSync('apps/dashboard/src/components/layout/WorkspaceSwitcher.tsx') ? 1 : 0)"`
- [x] AC-5: `ProjectsPage` foi deletada — Command: `node -e "process.exit(require('fs').existsSync('apps/dashboard/src/pages/ProjectsPage.tsx') ? 1 : 0)"`
- [x] AC-6: A página `/preferences` tem rota registrada — Command: `node -e "process.exit(require('fs').readFileSync('apps/dashboard/src/App.tsx','utf8').includes('/preferences') ? 0 : 1)"`
- [x] AC-7: i18n PT/EN paridade para chaves novas — Command: `node -e "const f=require('fs').readFileSync('apps/dashboard/src/i18n.ts','utf8');for(const k of ['sidebar.addProject','sidebar.projectMenu.uninstall','nav.preferences','preferences.language']){if(!f.includes('\"'+k+'\"'))process.exit(1)};process.exit(0)"`

## Plano

## Informações da Entidade

`ProjectEntry` (estado local do dashboard) — não está no entity-registry (registry tem 0 entities, é stack Rust + TS sem ORM). Campos atuais (preservados): `path: string` (absolute), `name: string`, `addedAt: string`. Persistido via `@tauri-apps/plugin-store` em `projects.json`, chave `"projects"`.

`ProjectDetection` (view derivada) — `{ installed: boolean, version: string | null }`, retornado pelo Tauri command `detect_project_mustard` e fetched per-path via `useProjectDetections()` (fan-out useQueries v5).

`activeWorkspaceId: string` no `useStore` — pivot consumido por `Home/Activity/Telemetry/Quality/Knowledge/Prd`. Bridge `addProject(path) → activateProject(path) → discoverProjects(path) → setActiveWorkspaceId(match.id)` já existe (b6 close), preservada.

## Arquivos

- `apps/dashboard/src-tauri/src/projects.rs` — extender com `uninstall_mustard(path)` (best-effort delete `.claude/` + `mustard.json`)
- `apps/dashboard/src-tauri/src/lib.rs` — registrar `projects::uninstall_mustard` no `invoke_handler!`
- `apps/dashboard/src/lib/projects.ts` — adicionar wrapper `uninstallMustard(path)`
- `apps/dashboard/src/components/layout/Sidebar.tsx` — rewrite para tree (substitui flat NavLink list + WorkspaceSwitcher mount + flat Workspace/Settings sections)
- `apps/dashboard/src/components/layout/WorkspaceSwitcher.tsx` — **DELETE** (a árvore substitui)
- `apps/dashboard/src/pages/ProjectsPage.tsx` — **DELETE** (a árvore é a lista)
- `apps/dashboard/src/components/projects/ProjectCard.tsx` — **DELETE** (substituído pelo nó da tree)
- `apps/dashboard/src/components/projects/AddProjectButton.tsx` — **DELETE** (lógica migra inline para Sidebar)
- `apps/dashboard/src/pages/Preferences.tsx` — **CREATE** (página nova com toggle de idioma)
- `apps/dashboard/src/pages/Settings.tsx` — remover card "Projetos", remover language toggle, manter env editor; strings → `t()`
- `apps/dashboard/src/App.tsx` — remover `<Route path="/projects" />` e import de ProjectsPage; adicionar `<Route path="/preferences" element={<Preferences />} />`
- `apps/dashboard/src/components/layout/Topbar.tsx` — LABELS: remover `/projects`, adicionar `/preferences`
- `apps/dashboard/src/i18n.ts` — adicionar chaves `sidebar.*`, `nav.preferences`, `preferences.*`; reorganizar `settings.*`; remover órfãs `projects.pageDescription/emptyTitle/emptyDescription/addButtonLabel/addTooltipDesktopOnly/actionOpen` e `settings.projectsCardTitle/projectsCardDescription/activeBadge/removeAction`

## Component Contract

**Sidebar.tsx** (rewrite — ~180 linhas estimadas)

Top-to-bottom:
- Logo/título + collapse toggle (preservar do header atual)
- `+ Adicionar projeto` (label `t('sidebar.addProject')`) — botão no topo da árvore com ícone `FolderPlus`. Click → `open({directory:true, multiple:false, title: t('projects.addDialogTitle')})` (já import existente do plugin-dialog) → `useProjectsStore.getState().addProject(path)`. Browser fallback: tooltip "Disponível no app desktop".
- **Tree de projetos** — loop sobre `useProjectsStore((s) => s.projects)`. Para cada projeto, renderiza `<ProjectTreeNode>`:
  - Header row (flex): chevron icon (`ChevronRight` quando colapsado, `ChevronDown` quando expandido) + status dot (verde=installed && !updateAvailable, amber=updateAvailable, cinza=!installed, spinner=isLoading) + project.name + spacer + kebab (`MoreVertical`, 14px)
  - Click no header (não no kebab): toggle expand local + `activateProject(path)` se ainda não for o ativo
  - Kebab dropdown (`@radix-ui/react-dropdown-menu` já dep via shadcn):
    - `t('sidebar.projectMenu.update')` (só se `detection.updateAvailable`) → `updateMustard(path)` + invalidate `['project-detection', path]`
    - `t('sidebar.projectMenu.uninstall')` (só se `detection.installed`) → confirm inline ou modal? Recomendado: confirm inline com toast — `uninstallMustard(path)` + invalidate
    - separator
    - `t('sidebar.projectMenu.removeFromRegistry')` (destructive) → `removeProject(path)`
  - Quando `isExpanded === true`, renderiza folhas (pl-6):
    - Home → `/` (Lang=pt: `t('nav.home')`)
    - Activity → `/activity`
    - Telemetry → `/telemetry`
    - Quality → `/quality`
    - Knowledge → `/knowledge`
    - Settings → `/settings`
  - Cada folha é um `<ProjectLeafLink to="/X" projectPath={path}>` que no click chama `activateProject(projectPath)` se ainda não for o ativo + `navigate(to)`. Active state visual via `useLocation()` casando path + isActive da prop pai (o projeto deve ser o ativo para a folha contar como ativa).
- Empty state (quando `projects.length === 0`): mostra "Nenhum projeto" (`t('sidebar.empty.title')`) + "Adicione uma pasta para começar" (`t('sidebar.empty.description')`) — exibido na própria sidebar, sem painel separado.
- Separator
- **Ferramentas globais** (preservar): `Commands`, `Prd` — links flat sem tree.
- Separator
- **Footer**: `⚙ Preferences` (label `t('nav.preferences')`) — NavLink para `/preferences` com ícone `Settings` (lucide-react).

Auto-expand: useEffect que, quando `activeWorkspaceId` muda OU quando a lista de projetos hidrata, expande o projeto cujo path resolve para o active workspace. Outros ficam fechados. Estado de expansão = `Map<path, boolean>` em useState local do Sidebar (in-memory por sessão).

**Preferences.tsx** (CREATE — ~80 linhas estimadas)

Página minimal:
- Header: breadcrumb + título `t('preferences.title')`
- Single card "Idioma" (`t('preferences.language')`) — dois botões `PT` / `EN` usando `useStore.language` + `setLanguage` (slice existente). Estados visuais consistentes com sibling `Settings.tsx`.
- TODO comment: "Futuras prefs (tema, telemetry opt-in) entram aqui."

Strings via `t()`. Layout segue o pattern `<PageHeader title={...} description={...} />` que outras páginas usam (verificar sibling).

**ProjectTreeNode** — componente interno do Sidebar.tsx (não exportado). Props: `{ project: ProjectEntry, detection: ProjectDetection | undefined, isActive: boolean, isExpanded: boolean, onToggleExpand: () => void }`.

## Tarefas

### Backend Tauri Agent (Wave 1)

- [x] Em `apps/dashboard/src-tauri/src/projects.rs`, adicionar `#[tauri::command] async fn uninstall_mustard(path: String) -> Result<(), String>`. Implementação: `std::fs::remove_dir_all(<path>/.claude)` ignorando `NotFound`; `std::fs::remove_file(<path>/mustard.json)` ignorando `NotFound`. Tipo de retorno consistente com `mustard_install`/`mustard_update` existentes (lib.rs:1453/1467).
- [x] Em `apps/dashboard/src-tauri/src/lib.rs`, registrar `projects::uninstall_mustard` no `tauri::generate_handler![...]` ao lado de `projects::detect_project_mustard` (linha ~1499).
- [x] Em `apps/dashboard/src/lib/projects.ts`, adicionar `uninstallMustard(path: string): Promise<void>` que invoca `'uninstall_mustard'` com `{ path }` via `@tauri-apps/api/core`. Mesmo padrão das siblings `installMustard`/`updateMustard`.
- [x] Verificar build Rust: `cargo check --manifest-path apps/dashboard/src-tauri/Cargo.toml`.

### Frontend UI Agent (Wave 2 — depende de Wave 1)

- [x] Adicionar chaves PT+EN em `apps/dashboard/src/i18n.ts` antes do refactor dos componentes:
  - `sidebar.addProject` ("Adicionar projeto" / "Add project")
  - `sidebar.projectMenu.update` ("Atualizar" / "Update")
  - `sidebar.projectMenu.uninstall` ("Remover Mustard" / "Uninstall Mustard")
  - `sidebar.projectMenu.removeFromRegistry` ("Remover do registry" / "Remove from registry")
  - `sidebar.empty.title` ("Nenhum projeto" / "No projects")
  - `sidebar.empty.description` ("Adicione uma pasta para começar" / "Add a folder to get started")
  - `sidebar.tools` ("Ferramentas" / "Tools")
  - `sidebar.status.installed` / `sidebar.status.updateAvailable` / `sidebar.status.notInstalled` / `sidebar.status.checking` (tooltips do status dot)
  - `nav.preferences` ("Preferences" / "Preferences")
  - `preferences.title` ("Preferences" / "Preferences")
  - `preferences.language` ("Idioma" / "Language")
  - `preferences.languagePt` ("Português" / "Portuguese")
  - `preferences.languageEn` ("Inglês" / "English")
  - `projects.toastUninstalled` ("Mustard removido de {{name}}" / "Mustard removed from {{name}}")
  - Remover chaves órfãs: `nav.projects`, `projects.pageDescription`, `projects.emptyTitle`, `projects.emptyDescription`, `projects.addButtonLabel`, `projects.addTooltipDesktopOnly`, `projects.actionOpen`, `settings.projectsCardTitle`, `settings.projectsCardDescription`, `settings.activeBadge`, `settings.removeAction`
- [x] Rewrite `apps/dashboard/src/components/layout/Sidebar.tsx` para o layout descrito em `## Component Contract`. Sub-componente `ProjectTreeNode` interno. Reuso: `useProjectsStore`, `useProjectDetections`, `open()` de plugin-dialog, ícones lucide-react já deps.
- [x] **DELETAR** `apps/dashboard/src/components/layout/WorkspaceSwitcher.tsx`.
- [x] **DELETAR** `apps/dashboard/src/pages/ProjectsPage.tsx`.
- [x] **DELETAR** `apps/dashboard/src/components/projects/ProjectCard.tsx`.
- [x] **DELETAR** `apps/dashboard/src/components/projects/AddProjectButton.tsx`.
- [x] Em `apps/dashboard/src/App.tsx`: remover import de `ProjectsPage` e a `<Route path="/projects" ... />`. Manter `useProjectsStore.getState().loadFromStore()` no bootstrap effect.
- [x] Em `apps/dashboard/src/components/layout/Topbar.tsx`: remover entrada `'/projects'` do LABELS map.

### Settings + Preferences Agent (Wave 3 — paralelo a Wave 2 onde possível)

- [x] CREATE `apps/dashboard/src/pages/Preferences.tsx` com a estrutura descrita em `## Component Contract`. Reusa o slice `useStore.language` + `setLanguage` (movido do Settings — confirmar sibling no Settings antes para preservar API).
- [x] Em `apps/dashboard/src/App.tsx`: adicionar `<Route path="/preferences" element={<Preferences />} />`.
- [x] Em `apps/dashboard/src/components/layout/Topbar.tsx`: adicionar `'/preferences': t('nav.preferences')` (ou string fixa se siblings usam string).
- [x] Em `apps/dashboard/src/pages/Settings.tsx`: remover o card "Projetos" inteiro (~50 linhas). Remover o card "Idioma" (toggle PT/EN — migra para Preferences). Manter o env editor por-active-project. Substituir strings hardcoded ("Environment", "Salvar mudanças", "Descartar") por `t('settings.envTitle')`, `t('settings.saveChanges')`, `t('settings.discardChanges')` (chaves já adicionadas no i18n.ts pela Wave 2).
- [x] Verificar que Settings ainda funciona sob `activeWorkspaceId` (env editor permanece scoped a `selectedProject`).

### Validate (após todas as waves)

- [x] `pnpm --filter mustard-dashboard build` passa sem erros TS.
- [x] `cargo check --manifest-path apps/dashboard/src-tauri/Cargo.toml` passa.
- [x] `git status` confirma: 4 files deleted, 5 files modified, 1 file created (Preferences.tsx). Total touch: 10 files.

## Dependências

- `b6-dashboard-projects` (CLOSED) — entrega o registry de projetos, a bridge `addProject → activateProject → setActiveWorkspaceId`, o Tauri `detect_project_mustard`, e os hooks `useProjectDetections` que esta spec consome.
- Sem novas npm deps. Sem novas crates Rust.
- shadcn `DropdownMenu` (já presente via `@radix-ui/react-dropdown-menu`).

## Limites

- `apps/dashboard/src/` (frontend TypeScript)
- `apps/dashboard/src-tauri/src/projects.rs` (extender)
- `apps/dashboard/src-tauri/src/lib.rs` (registrar command)
- **Fora dos limites:** `apps/cli/`, `apps/rt/`, `packages/core/`, demais apps. Páginas existentes (Home/Activity/Telemetry/Quality/Knowledge/Prd/Commands) não são modificadas — só a Sidebar muda como elas são acessadas.

## Preocupações

- O empty state da tree (sem projetos) precisa convidar o user a adicionar — não pode ser uma sidebar vazia confusa. Mitigação: `t('sidebar.empty.*')` com call-to-action visual no empty case.
- Auto-expand do projeto ativo deve casar mesmo se o user trocar de workspace via URL externa (deep link). Mitigação: useEffect na sidebar observa `activeWorkspaceId`.
- Uninstall via `fs::remove_dir_all` é destrutivo. Mitigação: kebab → confirm inline (pode reusar pattern de ProjectCard.tsx anterior antes da deleção) + toast de sucesso. Sem confirmação modal (consistente com `removeProject` que já remove sem modal).
- Settings sem language toggle pode confundir users que esperavam encontrá-lo lá. Mitigação: documentar no commit/PR; Settings continua sendo destino de env editor per-project, que é o seu papel coerente.
- Persistência do expand/collapse não é implementada (out-of-scope). Mitigação: auto-expand do ativo + folha-aberta da rota corrente cobrem 80% do caso. Persistência via plugin-store é trivial de adicionar depois se necessário.
- `analyze-validation` flaggou WARNs em `apps/dashboard/src/pages/Preferences.tsx` (falso positivo — está marcado `**CREATE**`) e `nav.preferences` (falso positivo — é chave i18n, não arquivo). Não-bloqueadores; documentados aqui.
