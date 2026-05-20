# Plan: b6 dashboard — pivot para Sidebar Treeview (email-account model)

## Context

A pipeline b6 entregou um registry de projetos e a bridge `addProject → activeWorkspaceId`, mas a sidebar continuou plana: `Projects` (lista) e `Home/Activity/Telemetry/Quality/Knowledge/Settings` (páginas-do-projeto-ativo) no mesmo nível, mais o `WorkspaceSwitcher` como dropdown separado. Isso é categoria-mista: o conceito "projeto" é meta, as outras são páginas-internas-do-projeto.

O user propôs (e confirmou via AskUserQuestion) pivotar a sidebar para um **treeview** análogo a clientes de e-mail multi-conta: cada projeto é um grupo expansível ("conta"), e dentro dele aparecem as mesmas folhas (`Home/Activity/Telemetry/Quality/Knowledge/Settings`, análogo a `Inbox/Sent/Drafts`). Click numa folha = `activateProject(path)` + `navigate('/activity')` num único gesto. O `WorkspaceSwitcher` some (a árvore É o switcher). A `ProjectsPage` some (a árvore É a lista; install/update/uninstall vão pro menu de contexto da raiz). `Settings` deixa de ser global e vira folha por-projeto. O toggle de idioma do dashboard (único setting hoje que é dashboard-global) migra para um `Preferences` num ícone gear no rodapé da sidebar.

Outcome desejado: a sidebar reflete a estrutura mental "cada projeto é um universo independente", com adicionar/remover/atualizar Mustard como ações da raiz de cada árvore, e settings/idioma do dashboard em footer separado fora da tree.

## Decisões fechadas (via AskUserQuestion)

- **Uninstall scope:** só deleta `.claude/` + `mustard.json`. Projeto fica como "Não instalado" no registry.
- **Two-button action per project:** "Abrir" (→ `/`) + "Configurações" (→ `/settings`) — mas isso agora vira navegação por folhas na tree.
- **Sidebar = treeview** (substitui flat sidebar + WorkspaceSwitcher).
- **Settings = folha por-projeto** na tree (não há mais Settings global).
- **Preferences (idioma do dashboard)** vai pro footer da sidebar, fora da tree.

## Anatomia da nova Sidebar

```
┌─────────────────────────────────────┐
│ Mustard                       ⇆     │  ← collapse toggle
├─────────────────────────────────────┤
│ Projetos               [+ Adicionar]│  ← raiz da tree
│                                     │
│ ● ProjetoA ▾                  [⋮]   │  ← installed=verde, kebab=Atualizar/Remover/Uninstall
│     Home                            │
│     Activity                        │
│     Telemetry                       │
│     Quality                         │
│     Knowledge                       │
│     Settings                        │
│                                     │
│ ○ ProjetoB ▸                  [⋮]   │  ← collapsed, not-installed=cinza
│                                     │
│ ● ProjetoC ▸                  [⋮]   │
├─────────────────────────────────────┤
│ Ferramentas                         │  ← global tools (existente, sem mudar)
│   Commands                          │
│   Prd                               │
├─────────────────────────────────────┤
│ ⚙ Preferences                       │  ← footer: modal/rota com idioma do dashboard
└─────────────────────────────────────┘
```

Comportamento:
- Click no nome do projeto (linha raiz): toggle expand/collapse + `activateProject(path)` (sem navegar)
- Click numa folha: `activateProject(path)` (se projeto não for o ativo) + `navigate('/<folha>')`
- Click no kebab `⋮` da linha raiz: dropdown com `Atualizar` (se updateAvailable), `Remover Mustard` (uninstall files), `Remover do registry` (separator + destructive)
- Click em `+ Adicionar`: file picker → `addProject(path)` (já ativa e expande automaticamente)
- Status dot: verde se `installed && !updateAvailable`, amber se `updateAvailable`, cinza se `!installed`, spinner se loading
- Projeto ativo: nome em bold + chevron expand auto + accent border-left

## Arquivos a modificar / criar / deletar

| File | Ação | Mudança |
|------|------|---------|
| `apps/dashboard/src/components/layout/Sidebar.tsx` | **Rewrite** | Substitui flat NavLink list por tree. Usa `useProjectsStore((s) => s.projects)` + `useProjectDetections()` (já existe) para status dot. Cada projeto = `<ProjectTreeNode>` interno com `useState` para expanded. Folhas: 6 NavLinks (Home/Activity/Telemetry/Quality/Knowledge/Settings) que disparam `activateProject(path)` antes de navegar. Tools group (Commands/Prd) preservado abaixo. Footer com `Preferences` gear. Total ~180 linhas; padrão `CollapsibleGroup.tsx` reusado/estendido. |
| `apps/dashboard/src/components/layout/Sidebar.tsx` (deleted parts) | **Remove** | `WorkspaceSwitcher` import e mount, flat Workspace/Tools/Settings sections, "Projects" nav entry (já que é a tree). |
| `apps/dashboard/src/components/layout/WorkspaceSwitcher.tsx` | **Delete** | A árvore substitui o dropdown. |
| `apps/dashboard/src/pages/ProjectsPage.tsx` | **Delete** | A árvore é a lista. Empty state ("nenhum projeto") aparece dentro da sidebar quando `projects.length === 0`. |
| `apps/dashboard/src/components/projects/ProjectCard.tsx` | **Delete** | Substituído pelo nó da tree + kebab. |
| `apps/dashboard/src/components/projects/AddProjectButton.tsx` | **Move/inline** | A lógica do `open({directory:true})` + `addProject(path)` migra para um helper inline no Sidebar.tsx (~20 linhas, sem necessitar de componente reusável). Pode ficar como utility em `lib/projects.ts` se preferir DRY com Preferences. |
| `apps/dashboard/src/hooks/useProjectDetections.ts` | **Keep, reuse** | Sidebar agora consome este hook diretamente. |
| `apps/dashboard/src/App.tsx` | **Edit** | Remover `<Route path="/projects" />` (página deletada). Manter todas as outras rotas (Home, Activity, Telemetry, Quality, Knowledge, Settings, Commands, Prd, project/:id, etc.). Adicionar `<Route path="/preferences" element={<Preferences />} />`. Remover import de ProjectsPage. Manter `useProjectsStore.getState().loadFromStore()` no bootstrap. |
| `apps/dashboard/src/pages/Preferences.tsx` | **Create** | Nova página simples. Contém o language toggle (mesma lógica do que estava em Settings) e placeholder para futuros settings dashboard-global. Strings via `t()`. |
| `apps/dashboard/src/pages/Settings.tsx` | **Edit** | Remover card "Projetos" (já não pertence aqui) E remover language toggle (vai pra Preferences). Manter SÓ o env editor (per-active-project). Replace strings hardcoded ("Environment", "Salvar mudanças", "Descartar") por `t()`. Página agora tem 1 seção principal. |
| `apps/dashboard/src/components/layout/Topbar.tsx` | **Edit** | Remover `'/projects'` da LABELS map. Adicionar `'/preferences': t('nav.preferences')`. |
| `apps/dashboard/src-tauri/src/projects.rs` | **Extend** | Adicionar `#[tauri::command] async fn uninstall_mustard(path: String) -> Result<(), String>` — `fs::remove_dir_all(<path>/.claude)` (ignora NotFound) + `fs::remove_file(<path>/mustard.json)` (ignora NotFound). Best-effort. |
| `apps/dashboard/src-tauri/src/lib.rs` | **Edit** | Registrar `projects::uninstall_mustard` no `invoke_handler!` (linha ~1499, ao lado de detect_project_mustard). |
| `apps/dashboard/src/lib/projects.ts` | **Edit** | Adicionar `uninstallMustard(path: string): Promise<void>` (wrapper `invoke('uninstall_mustard', { path })`). |
| `apps/dashboard/src/lib/projects-store.ts` | **Keep** | A bridge `addProject → activateProject → setActiveWorkspaceId` continua intacta. |
| `apps/dashboard/src/i18n.ts` | **Edit** | Adicionar PT+EN: `sidebar.addProject` ("Adicionar projeto" / "Add project"), `sidebar.projectMenu.update`, `sidebar.projectMenu.uninstall` ("Remover Mustard" / "Uninstall Mustard"), `sidebar.projectMenu.removeFromRegistry` ("Remover do registry" / "Remove from registry"), `sidebar.empty.title` ("Nenhum projeto" / "No projects"), `sidebar.empty.description` ("Adicione uma pasta para começar" / "Add a folder to get started"), `sidebar.tools` ("Ferramentas" / "Tools"), `nav.preferences` ("Preferences" / "Preferences"), `preferences.title`, `preferences.language` ("Idioma" / "Language"), `preferences.languagePt` ("Português" / "Portuguese"), `preferences.languageEn` ("Inglês" / "English"), `settings.envTitle` ("Environment" / "Environment"), `settings.saveChanges` ("Salvar mudanças" / "Save changes"), `settings.discardChanges` ("Descartar" / "Discard"), `projects.toastUninstalled` ("Mustard removido de {{name}}" / "Mustard removed from {{name}}"). Remover chaves órfãs: `nav.projects`, `projects.pageDescription`, `projects.emptyTitle`, `projects.emptyDescription`, `projects.addButtonLabel`, `projects.addTooltipDesktopOnly`, `projects.actionOpen`, `projects.statusInstalled` etc. relacionadas a ProjectCard (consolidar: status dot não tem texto, só cor + tooltip via `t('sidebar.status.installed')` etc.). |

## Reuso (já existe — não recriar)

- `useProjectsStore` em `lib/projects-store.ts` — toda a API (`addProject`/`removeProject`/`activateProject`).
- `useProjectDetections()` em `hooks/useProjectDetections.ts` — fan-out por path com queryKey existente.
- `discoverProjects` em `api/discovery.ts` — usado por `activateProject` para resolver path → id; **não tocar**.
- `useStore.setActiveWorkspaceId` em `lib/store.ts` — bridge target, mantém-se.
- `CollapsibleGroup.tsx` — padrão (useState + ChevronRight/ChevronDown). Estendível para tree node (ou apenas seguir o mesmo idioma DIY).
- Ícones `lucide-react` (já dep): `FolderPlus`, `ChevronRight`, `ChevronDown`, `MoreVertical`, `Trash2`, `Settings`, `Home`, `Activity`, `BarChart3`, `Award`, `BookOpen`, `Circle` (status dot), `Loader2` (loading dot).
- `DropdownMenu` primitives (shadcn) — usado em WorkspaceSwitcher; mover para o kebab da tree node.
- `@tauri-apps/plugin-dialog`'s `open()` — pattern reusado do AddProjectButton (que vai sumir; lógica vai inline).
- `queryClient.invalidateQueries({ queryKey: ['project-detection', path] })` — após uninstall, atualiza status dot.

## Layout / Interaction details

**ProjectTreeNode (componente interno do Sidebar.tsx)**

Props: `{ project: ProjectEntry, detection: ProjectDetection | undefined, isActive: boolean }`.

```tsx
<div className="project-node">
  <div className="project-header" onClick={togglerExpand}>
    <ChevronIcon expanded={isExpanded} />
    <StatusDot variant={statusFromDetection(detection)} />
    <span className={isActive ? 'font-medium' : ''}>{project.name}</span>
    <DropdownMenu> {/* kebab */}
      <DropdownMenuTrigger><MoreVertical size={14} /></DropdownMenuTrigger>
      <DropdownMenuContent>
        {detection?.updateAvailable && <DropdownMenuItem onSelect={() => handleUpdate(project.path)}>{t('sidebar.projectMenu.update')}</DropdownMenuItem>}
        {detection?.installed && <DropdownMenuItem onSelect={() => handleUninstall(project.path)}>{t('sidebar.projectMenu.uninstall')}</DropdownMenuItem>}
        <DropdownMenuSeparator />
        <DropdownMenuItem destructive onSelect={() => removeProject(project.path)}>{t('sidebar.projectMenu.removeFromRegistry')}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
  {isExpanded && (
    <div className="project-leaves pl-6">
      <ProjectLeafLink to="/" icon={Home} label={t('nav.home')} projectPath={project.path} />
      <ProjectLeafLink to="/activity" icon={Activity} label={t('nav.activity')} projectPath={project.path} />
      {/* ... */}
      <ProjectLeafLink to="/settings" icon={Settings} label={t('nav.settings')} projectPath={project.path} />
    </div>
  )}
</div>
```

`ProjectLeafLink` (helper): on click → `useProjectsStore.getState().activateProject(projectPath)` then `navigate(to)`. Use `useLocation()` para destacar a folha ativa (somente quando `isActive === true` da prop pai). Match path-equality.

Auto-expand: quando `addProject` resolve, o useEffect de Sidebar detecta o novo entry com `path === activeWorkspaceId` e expande automaticamente (set local expanded state). Mesma regra na hidratação inicial: o projeto ativo abre, os outros ficam fechados.

Persist expanded state: in-memory only para MVP (não persiste entre reloads do dashboard). TODO no plano se quiser persistir depois.

## Verificação end-to-end

1. **Build:** `pnpm --filter mustard-dashboard build` → `tsc -b && vite build` passa sem erros.
2. **Cargo check:** `cargo check --manifest-path apps/dashboard/src-tauri/Cargo.toml` passa (novo Tauri command compila).
3. **Manual no dev:** `pnpm --filter mustard-dashboard tauri:dev`:
   - Sidebar mostra a tree (ou empty state se 0 projetos)
   - "+ Adicionar projeto" abre file picker → projeto entra na lista, expande, vira ativo, status dot reflete detection
   - Click em folha (Activity) → workspace ativa (se já não estava) + navega para `/activity` + página mostra dados do projeto
   - Kebab → Atualizar / Remover Mustard / Remover do registry → cada ação completa e status dot atualiza (via invalidate)
   - Uninstall: `.claude/` + `mustard.json` do path somem (verificar com explorer/fs); projeto continua na sidebar como cinza
   - Settings (folha): mostra env editor do projeto ativo, sem card "Projetos", sem language toggle
   - Footer "Preferences" → modal/página com language toggle (PT/EN alterna i18n)
   - WorkspaceSwitcher antigo não aparece mais em lugar nenhum
   - `/projects` não está mais acessível (route removida)
4. **i18n parity:** PT e EN espelhadas em `i18n.ts`. Grep deve casar; chaves órfãs removidas.
5. **Boundary check:** zero modificações em `pages/Home.tsx`, `Activity.tsx`, `Telemetry.tsx`, `Quality.tsx`, `Knowledge.tsx`, `Prd.tsx`, `Commands.tsx`, `ProjectDetail.tsx`, `SpecDetail.tsx`, `hooks/`, `api/`. Tree só altera nav; as pages seguem usando activeWorkspaceId.

## Hard constraints (do CLAUDE.md do dashboard)

- HashRouter only.
- `useQueries` keyed por `project.path` no `useProjectDetections` — manter.
- Slice selectors no zustand.
- Todo `invoke()` em `lib/projects.ts`.
- Triple-touch quando muda rota: aqui removemos `/projects` (Topbar atualiza) e adicionamos `/preferences` (Topbar + Sidebar footer).
- Source code em inglês.
- Sem novas dependências npm. Tree é DIY com `useState` + ícones já existentes em `lucide-react`.

## Out of scope

- Persistência do estado expand/collapse entre reloads (TODO comment, in-memory por enquanto).
- Drag-reorder de projetos na tree.
- Sub-grupos dentro de um projeto (ex.: Monitoring/Reference como sub-grupos das folhas) — folhas ficam flat sob cada projeto.
- Tema do dashboard (futuro Preferences).
- Comando `mustard-cli uninstall` (CLI binary) — Tauri faz `fs::remove_*` direto.
- Rewire de `/project/:id` e `/project/:id/spec/:specName` para usar path-as-id em vez de hash-id (legado, mantém como está).
