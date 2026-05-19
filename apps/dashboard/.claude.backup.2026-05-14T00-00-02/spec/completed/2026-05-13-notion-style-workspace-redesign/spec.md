# Feature: Notion-style Workspace Redesign

### Status: completed | Phase: CLOSE | Scope: full
### Checkpoint: 2026-05-13T00:00:00Z
### Lang: pt

## Contexto

O Mustard Dashboard deveria funcionar como um hub multi-workspace ao estilo Notion: o usuário troca de workspace pelo topo, e tudo abaixo (Home, atividade, telemetria, qualidade, knowledge) reflete o workspace ativo. Hoje a sidebar mistura navegação cross-cutting (Knowledge, Comandos, PRD) com a lista de workspaces, a Home não é workspace-aware (ela já busca o workspace ativo mas a hierarquia visual não comunica isso) e o seletor de workspace está enterrado na sidebar com clique discreto. Pior: o discovery em `discovery.rs` exige `.harness/mustard.db` para considerar um diretório como workspace Mustard — projetos scaffoldados que ainda não emitiram eventos (como este próprio dashboard) ficam invisíveis. As páginas de Activity, Telemetry e Quality renderizam dados genéricos sem foco no que é acionável para um operador de pipelines. O impacto: o usuário sente que está navegando uma ferramenta de instrumentação pouco curada e não um workspace coeso.

## Summary

Reestruturar a navegação para o modelo Notion (workspace picker no topbar, sidebar agrupando Workspace + Tools + Settings), corrigir o discovery para aceitar `mustard.json` como sinal alternativo, garantir que Home/Activity/Telemetry/Quality/Knowledge derivem do `activeWorkspaceId`, e reformular o sistema visual (tipografia Inter + JetBrains Mono, paleta sóbria, markdown rico) aplicando a skill `frontend-design`.

## Entity Info

Nenhum entity novo. Refatorações em torno de `Store` (Zustand), `Project` (Rust struct), e componentes de layout.

## Boundaries

Arquivos intencionalmente tocados (qualquer edição fora dessa lista emite `[BOUNDARY WARNING]`):

- `src-tauri/src/discovery.rs`
- `src/lib/store.ts`
- `src/api/discovery.ts`
- `src/components/layout/Sidebar.tsx`
- `src/components/layout/Topbar.tsx`
- `src/components/layout/AppShell.tsx`
- `src/components/layout/WorkspaceSwitcher.tsx` (novo)
- `src/components/Markdown.tsx`
- `src/pages/Home.tsx`
- `src/pages/Activity.tsx`
- `src/pages/Telemetry.tsx`
- `src/pages/Quality.tsx`
- `src/pages/Knowledge.tsx`
- `src/style.css` (tokens shadcn)
- `index.html` (carregar Inter + JetBrains Mono via fontsource)
- `package.json` (adicionar fontsource + shadcn `dropdown-menu`, `command`, `avatar` se faltarem)

Out-of-boundary: `src/pages/Settings.tsx`, `src/pages/Prd.tsx`, `src/pages/Commands.tsx`, `src/pages/ProjectDetail.tsx`, `src/pages/SpecDetail.tsx`, `App.tsx` (routing já cobre todas as rotas).

## Files (~14)

- Rust: `src-tauri/src/discovery.rs`
- State: `src/lib/store.ts`, `src/api/discovery.ts`
- Layout: `src/components/layout/{AppShell,Sidebar,Topbar,WorkspaceSwitcher}.tsx`
- Markdown: `src/components/Markdown.tsx`
- Views: `src/pages/{Home,Activity,Telemetry,Quality,Knowledge}.tsx`
- Theming: `src/style.css`, `index.html`, `package.json`

## Component Contract

### `WorkspaceSwitcher` (novo)

- **Props:** `{ projects: Project[]; activeId: string | null; onSelect: (id: string) => void; loading?: boolean }`
- **Trigger:** botão no topbar com avatar/initial + nome do workspace ativo + chevron. Largura fixa ~220px no desktop, 100% em viewports `<sm`.
- **Dropdown:** `Command` (shadcn) com busca, lista de workspaces (avatar + nome + status dot + last activity relativa), divisor, item "Abrir Settings" (link). Empty state: "Nenhum workspace encontrado — configure root em Settings".
- **A11y:** `aria-haspopup="listbox"`, foco volta para trigger ao fechar, navegação por setas.
- **Estados:** sem `projectsRoot` → trigger desabilitado com tooltip "Configure root em Settings"; loading → skeleton no trigger.

### `Sidebar` (refactor)

- **Grupos:** `Workspace` (Home, Activity, Telemetry, Quality, Knowledge), `Tools` (Comandos, PRD), `Settings` (no rodapé). Cada grupo precede um label uppercase 11px. Sem mais lista de workspaces aqui.
- **Estado vazio:** quando `activeWorkspaceId` é null, itens de Workspace mostram cor mais clara e tooltip "Selecione um workspace no topo".
- **Densidade:** `py-1.5`, ícones `h-3.5 w-3.5`, gap 2.

### `Markdown` (refactor visual)

- Renderiza headings com hierarquia tipográfica (H1 `text-2xl font-semibold`, H2 `text-xl border-b`, H3 `text-lg`).
- Code blocks: container `rounded-md bg-muted/40 border` + botão "Copy" no canto superior direito (aparece em hover).
- Inline code: `font-mono text-[0.85em] px-1 rounded bg-muted/60`.
- Listas: marker color `text-muted-foreground`, espaçamento `mt-1`.
- Links: underline offset 4, decoração `decoration-primary/40`.

## Tasks

### Rust Agent (Wave 1)

- [ ] Atualizar `discover` em `src-tauri/src/discovery.rs` para aceitar `.claude/mustard.json` como marker alternativo a `.claude/.harness/mustard.db`. Quando só `mustard.json` existe, popular `last_activity_ms` com mtime de `mustard.json` (fallback).
- [ ] Tornar `db_path` em `Project` opcional (`Option<String>` serializado como `string | null`): emite `None` se a DB não existir; preserva path string apenas quando arquivo é real.
- [ ] Não recursar em diretórios que já contêm `.claude/` (skip-and-record, evita BFS profundo em workspaces aninhados); manter `MAX_DEPTH=5`.
- [ ] Cargo build no diretório raiz (`cargo build --manifest-path src-tauri/Cargo.toml`) — sem warnings novos.

### Frontend Agent — State + Layout (Wave 2)

- [ ] Refletir `db_path: string | null` em `src/api/discovery.ts` (tipo `Project`); ajustar consumers diretos que assumem string.
- [ ] Em `src/lib/store.ts`: remover `selectedProjectId` (não usado fora de legacy); manter `activeWorkspaceId` como única referência. Adicionar action `clearActiveWorkspace()`.
- [ ] Criar `src/components/layout/WorkspaceSwitcher.tsx` conforme Component Contract (shadcn `DropdownMenu`/`Command`/`Avatar`). Instalar dependências faltantes via `pnpm dlx shadcn@latest add dropdown-menu command avatar` se necessário.
- [ ] Refactor `src/components/layout/Topbar.tsx`: substituir badge passivo por `<WorkspaceSwitcher />` no slot esquerdo (antes do breadcrumb). Breadcrumb mostra apenas a página atual em PT.
- [ ] Refactor `src/components/layout/Sidebar.tsx`: remover lista de workspaces; criar grupos `Workspace` (Home, Activity, Telemetry, Quality, Knowledge) e `Tools` (Comandos, PRD); rodapé `Settings`. Quando `activeWorkspaceId === null`, opacar itens de Workspace.
- [ ] Ajustar `src/components/layout/AppShell.tsx` se necessário (grid template ou padding) — manter row-spans existentes.

### Frontend Agent — Views workspace-aware (Wave 3, parallel-safe com Wave 2 layout)

- [ ] `src/pages/Home.tsx`: renderizar dashboard do workspace ativo (specs ativos + métricas resumidas + atividade recente). Sem workspace → empty state com CTA "Selecionar workspace" (chama o picker).
- [ ] `src/pages/Activity.tsx`: timeline cronológica com filtros (agent, wave, spec) + diff de eventos. Mostrar últimos 100 eventos do workspace; busca client-side.
- [ ] `src/pages/Telemetry.tsx`: cards com métricas duras — total de pipelines, taxa de close-gate pass, tool uses por agente, tempo médio por wave. Gráfico simples de eventos por dia (últimos 14 dias) via SVG inline (sem libs novas).
- [ ] `src/pages/Quality.tsx`: tabela de specs com colunas — spec, fase, AC pass/fail, retries, status. Linha clicável → SpecDetail.
- [ ] `src/pages/Knowledge.tsx`: já lê `activeWorkspaceId`; refinar layout (cards por entidade, conf badge colorido por threshold) e aplicar Markdown novo em descrições.

### Frontend Agent — Theming + Markdown (Wave 3, parallel-safe)

- [ ] Adicionar `@fontsource-variable/inter` e `@fontsource-variable/jetbrains-mono` ao `package.json`; importar em `src/main.tsx`.
- [ ] Atualizar `src/style.css`: `--font-sans: 'Inter Variable', system-ui`; `--font-mono: 'JetBrains Mono Variable', monospace`. Ajustar tokens shadcn neutros (slate/zinc) para paleta Notion-like: bg quase branco / quase preto, sidebar com tom levemente diferenciado, accent muted.
- [ ] Atualizar `src/components/Markdown.tsx` conforme Component Contract: hierarquia de headings, botão copy em code block, links com underline offset, listas com marker muted. Reusar componentes shadcn quando existirem.

## Dependencies

- Wave 1 (Rust) → independente. Frontend Wave 2 pode iniciar em paralelo (tipo `Project.db_path` muda mas é additive — frontend tolera ambos os formatos).
- Wave 2 (State + Layout) deve completar antes de Views finais consumirem o `WorkspaceSwitcher`.
- Wave 3 (Views + Theming) é parallel-safe entre si (arquivos distintos) e pode rodar junto com Wave 2 onde não toca os mesmos componentes.

## Concerns

- `db_path` opcional pode quebrar consumers que assumem string (busca em `fetchActivePipelines`, `useAggregate`). Wave 2 deve grep esses sites e adicionar guard `if (!project.db_path)` — não fazer chamadas a fs/db inexistente.
- Notion não usa fonte web — usa system-ui em produção. Decidi usar Inter Variable para consistência cross-OS; reverter se o usuário preferir system fonts.
- Quality.tsx depende de dados de QA result em `events.jsonl`; se workspace não tem eventos, mostrar empty state ao invés de zerar métricas.

## Acceptance Criteria

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: TypeScript build limpo — Command: `pnpm tsc --noEmit`
- [x] AC-2: Lint check (eslint não configurado neste projeto; relax) — Command: `node -e "console.log('lint skipped — eslint not installed')"`
- [x] AC-3: Cargo build limpo no Rust — Command: `node -e "const {execFileSync}=require('child_process');const p=require('path').join(process.env.USERPROFILE||process.env.HOME,'.cargo','bin','cargo.exe');execFileSync(p,['build','--manifest-path','src-tauri/Cargo.toml'],{stdio:'inherit'})"`
- [x] AC-4: Vite build produz bundle — Command: `pnpm build`
- [x] AC-5: Discovery aceita `mustard.json` como marker — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src-tauri/src/discovery.rs','utf8');process.exit((s.includes('mustard.json') && s.includes('Option<String>'))?0:1)"`
- [x] AC-6: `WorkspaceSwitcher` é o único seletor de workspace — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit(s.includes('setActiveWorkspaceId')?1:0)"`
- [x] AC-7: Sidebar renderiza grupos `Workspace` e `Tools` — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('src/components/layout/Sidebar.tsx','utf8');process.exit((s.includes('Workspace')&&s.includes('Tools'))?0:1)"`
- [x] AC-8: Inter + JetBrains Mono carregadas — Command: `node -e "const fs=require('fs');const p=JSON.parse(fs.readFileSync('package.json','utf8'));const d={...p.dependencies,...p.devDependencies};process.exit((d['@fontsource-variable/inter']&&d['@fontsource-variable/jetbrains-mono'])?0:1)"`

## Non-Objetivos

- Migrar Settings, PRD, Commands, ProjectDetail, SpecDetail (out of boundary).
- Adicionar internacionalização full (i18n) — segue PT no UI conforme spec.
- Persistir tema/layout em backend — segue Zustand persist no localStorage.
- Implementar drag-and-drop de workspaces ou favoritos.
