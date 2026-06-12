# redesenho da rota visao geral do dashboard: cards de specs por estagio com navegacao filtrada, faixa de alertas (suspeitas e specs paradas), cards de projeto (monorepo linguagem), info git local e arquivos mais tocados, removendo ROI economia e timeline

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A rota "Visão Geral" (`AggregateOverview`) hoje mostra informação redundante e pouco usada: 4 KPIs soltos, um placar de retorno-sobre-investimento (ROI = Return On Investment), o bloco agregado de Consumo & Economia (que já tem página própria, `Economia.tsx`), os pipelines ativos e uma linha-do-tempo de atividade recente. O usuário quer enxugar a rota para duas seções com propósito claro e reimplementar o resto depois, com mais critério.

**Decisões de produto já tomadas com o usuário** (elicitação na fase ANALYZE):
- **Cards de spec por estágio + faixa de Alertas** (opção escolhida): os cards de status mostram o *estágio* do ciclo de vida (Planejando · Executando · Finalizadas) e os sinais de atenção (Suspeitas, Specs paradas) ficam numa faixa de **Alertas** separada — porque "Suspeitas" é um alerta, não um estágio.
- **"Suspeitas"** = specs ainda ativas que dispararam um evento de higiene (`hygiene.detected`) nos últimos 7 dias (ex.: wave que falhou, pipeline travado, gate órfão). Fonte: `workspace_health().suspects` + `suspect_specs`.
- **Informação de GitHub = git local** (remote, branch, ahead/behind, último commit), via comando Tauri local rodando `git`. Sem rede nem autenticação `gh`.

**O que já existe (recompor) vs net-new** — verificado lendo as âncoras reais (re-consulta forte ao digest):
- Specs por status: `WorkspaceSpecsByStatus` já existe, mas navega para `/specs` sem filtro → estender para navegação filtrada.
- Filtro da página de specs: hoje só 3 buckets (`ativas`/`suspeitas`/`encerradas`, `Specs.tsx:192`) → adicionar sub-filtro de estágio (Planejando/Executando/Finalizadas).
- `SpecCard` já carrega `phase`, `status` e `last_event_at` (`dashboard.ts`) → estágios e detecção de "parada" (stale) são deriváveis no front, sem backend novo.
- Info de projeto: `dashboard_subprojects` (`lib.rs:446`) hoje expõe só `{name, role}` → estender para devolver linguagens/stacks/monorepo do `.claude/grain.model.json` (que `read_projects()` já carrega).
- Arquivos mais tocados: `WorkspaceFilesRanking` + `top_files` já existem → reusar.
- **Git local: NET-NEW** — nada de git é coletado hoje; comando Tauri novo.

Âncoras reais (re-consulta forte, alta confiança — confirmadas na leitura):
- apps/dashboard/src/features/workspace/AggregateOverview/index.tsx (rota a redesenhar)
- apps/dashboard/src/features/workspace/WorkspaceSpecsByStatus/index.tsx (specs por status — reusar/estender)
- apps/dashboard/src/features/workspace/WorkspaceFilesRanking/index.tsx (arquivos mais tocados — reusar)
- apps/dashboard/src/pages/Specs.tsx (filtros — estender estágio/stale)
- apps/dashboard/src/lib/dashboard.ts (bindings Tauri + tipos)
- apps/dashboard/src/hooks/useProject.ts (hook de projeto)
- apps/dashboard/src-tauri/src/lib.rs (registro de comandos Tauri + structs)
- apps/dashboard/src-tauri/src/spec_views.rs (WorkspaceHealth, SpecCard, suspects)
- apps/scan/src/model.rs (ProjectModel: languages/stacks/frameworks)

**Por que agora**: a rota é a porta de entrada do dashboard; hoje ela dilui atenção em métricas que pertencem a outras páginas e esconde os dois sinais que o usuário realmente quer ver de relance — saúde das specs e identidade do projeto.

## Usuários/Stakeholders

Desenvolvedor que abre o dashboard mustard para ter, num relance, o estado do trabalho (specs) e a identidade do repositório (projeto). É a primeira tela após escolher um workspace — precisa responder "o que está em andamento e o que merece atenção?" e "que repositório é este?" sem cliques extras.

## Métrica de sucesso

A rota "Visão Geral" passa a conter **apenas** duas seções (Specs e Projetos), sem ROI/Economia/Timeline. Cada card de estágio e cada alerta abre a página `/specs` já no filtro correto em **um clique**. O card de Projetos mostra monorepo + nº de projetos + linguagens, o card de Git mostra branch/remote/ahead-behind/último commit, e os arquivos mais tocados aparecem reusando o componente existente. Build e lint do dashboard e do workspace Rust permanecem verdes.

## Não-Objetivos

- **Não** reimplementar ROI/Consumo/Economia/Timeline nesta rota — serão repensados depois, com mais critério (a página `Economia.tsx` segue dona do detalhe de consumo).
- **Não** integrar a API remota do GitHub (PRs/issues/`gh`): apenas git local. Decisão explícita do usuário.
- **Não** criar novo cálculo de churn por git para "arquivos mais tocados" — reusar o ranking existente (`top_files`, contagem de `tool.use`).
- **Não** persistir o limiar de "spec parada" em configuração — fica uma constante (7 dias) revisável depois.
- **Não** mexer em superfícies agnósticas do core/scan para coletar git (git local mora no backend do dashboard, `src-tauri`).

## Critérios de Aceitação

- **AC-1** — O workspace Rust compila com o comando Tauri de git local e os structs novos.
  Command: `cargo build --workspace`

- **AC-2** — O dashboard (frontend + bindings) compila.
  Command: `pnpm --filter mustard-dashboard build`

- **AC-3** — Lint limpo no dashboard.
  Command: `pnpm --filter mustard-dashboard lint`

- **AC-4** — O comando Tauri `dashboard_git_info` existe e está registrado no `invoke_handler`.
  Command: `grep -rn "dashboard_git_info" apps/dashboard/src-tauri/src/lib.rs`

- **AC-5** — A info de projeto (linguagens/monorepo) é exposta ao front pelo backend (campo além de name/role).
  Command: `grep -rnE "languages|frameworks|monorepo|project_count" apps/dashboard/src-tauri/src`

- **AC-6** — A rota Visão Geral não referencia mais os widgets removidos (ROI, Economia agregada, Timeline).
  Command: `grep -qE "RoiScoreboard|RecentActivity" apps/dashboard/src/features/workspace/AggregateOverview/index.tsx && exit 1 || exit 0`

<!-- PLAN -->

## Arquivos

**Backend — Tauri (apps/dashboard/src-tauri) — Onda 1:**
- `apps/dashboard/src-tauri/src/git_info.rs` (criar) — comando que roda `git` local e devolve `{remote, branch, ahead, behind, last_commit}`; estado vazio quando não há repositório/remote.
- `apps/dashboard/src-tauri/src/project_overview.rs` (criar) — lê `.claude/grain.model.json` via `mustard_core::read_projects()` e projeta `{is_monorepo, project_count, languages, frameworks, detected_stacks}`.
- `apps/dashboard/src-tauri/src/lib.rs` — EDITAR — declarar os módulos, definir/registrar os comandos `dashboard_git_info` e `dashboard_project_overview` no `invoke_handler` + structs serde (camelCase → snake_case).

**Frontend — React/TS (apps/dashboard/src) — Onda 2:**
- `apps/dashboard/src/lib/dashboard.ts` — EDITAR — bindings `fetchGitInfo` e `fetchProjectOverview` + tipos `GitInfo`, `ProjectOverview`.
- `apps/dashboard/src/hooks/useGitInfo.ts` (criar) — hook TanStack Query sobre `fetchGitInfo` (gate `enabled: !!repoPath`, queryKey com repoPath na folha).
- `apps/dashboard/src/hooks/useProjectOverview.ts` (criar) — hook sobre `fetchProjectOverview`.
- `apps/dashboard/src/features/workspace/SpecStatusCards/index.tsx` (criar) — 3 cards de estágio (Planejando/Executando/Finalizadas); contagens derivadas de `fetchSpecCards`; clique navega para `/specs?filter=<estágio>`.
- `apps/dashboard/src/features/workspace/SpecAlertsBand/index.tsx` (criar) — faixa de Alertas: Suspeitas (de `workspace_health`) + Specs paradas (stale, derivado de `last_event_at`, limiar 7 dias); clique navega filtrado.
- `apps/dashboard/src/features/workspace/ProjectInfoCard/index.tsx` (criar) — monorepo + nº de projetos + linguagens/stacks (de `useProjectOverview`).
- `apps/dashboard/src/features/workspace/GitInfoCard/index.tsx` (criar) — branch/remote/ahead-behind/último commit (de `useGitInfo`).
- `apps/dashboard/src/features/workspace/AggregateOverview/index.tsx` — EDITAR (grande) — remover ROI/Consumo&Economia/4-KPIs/Timeline; reestruturar em seção **Specs** (SpecStatusCards + SpecAlertsBand) e seção **Projetos** (ProjectInfoCard + GitInfoCard + WorkspaceFilesRanking reusado).
- `apps/dashboard/src/pages/Specs.tsx` — EDITAR — ler params de estágio (`planejando`/`executando`/`finalizadas`) e `stale`; derivar estágio de `SpecCard.phase/status`; mapear para bucket + sub-filtro.

Censo: ~12 arquivos, 7 criados / 5 editados, 2 camadas (backend Rust Tauri + frontend React), 1 comando net-new + projeção net-new + 4 componentes net-new.

## Limites

IN:
- Rota `AggregateOverview` e seus novos componentes filhos (Specs + Projetos).
- 2 comandos Tauri no backend do dashboard (`dashboard_git_info`, `dashboard_project_overview`) + bindings/hooks.
- Extensão dos filtros de `Specs.tsx` para estágio + stale.

OUT:
- `core`/`scan` (`packages/core`, `apps/scan`, `apps/rt`) — não tocar; o backend do dashboard apenas **lê** `.claude/grain.model.json` pela API pública `read_projects()`.
- Página `Economia.tsx` e o pipeline de telemetria de consumo — permanecem como estão.
- API remota do GitHub (`gh`/rede) — fora de escopo.
- Persistência de configuração do limiar de stale.

## Concerns

Levantadas no REVIEW (não-bloqueantes; verdito = aprovado, sem defeito de código):
- **AC-5 corrigido:** o comando original fazia grep em `lib.rs`, mas os campos (`languages`/`frameworks`/`is_monorepo`/`project_count`) vivem em `project_overview.rs` (módulo separado — estrutura SOLID correta, registrado em `lib.rs:9`/`:3029`). Comando retargetado para o diretório `apps/dashboard/src-tauri/src` (acha o módulo; verificado exit 0).
- **AC-6 corrigido (cross-shell):** o `qa-run` executa os comandos no **cmd.exe** do Windows, onde `!` (negação) e `&` (em "Consumo & Economia") são bash-ismos inválidos. Reescrito para `grep -qE "RoiScoreboard|RecentActivity" <arquivo> && exit 1 || exit 0` (operadores `&&`/`||` válidos em cmd E bash; ausente→exit 0, presente→exit 1; verificado sob cmd.exe). A ausência de "Consumo & Economia" foi confirmada pelo REVIEW.
- **AC-1/AC-2 são gates fracos para o backend:** `cargo build --workspace` **exclui** `apps/dashboard/src-tauri` (root `Cargo.toml:8`) e o build do dashboard é frontend-only — nenhum dos dois compila o crate Tauri. A compilação real do backend foi confirmada pelo reviewer via `cargo check` dentro de `src-tauri` (exit 0) + testes (git_info 2/2, project_overview 1/1). Uma melhoria futura seria o AC-1 chamar `cargo check` no crate `src-tauri` (cuidado com o lock do .exe no Windows — usar `CARGO_TARGET_DIR` isolado).
- **Linguagem = `kind`:** `ProjectOverview.languages` carrega o `kind` de cada projeto (cargo/npm/go), pois o modelo não tem campo de linguagem; `ProjectInfoCard` mapeia kind→rótulo amigável. Sem dado fabricado.
- **i18n órfão:** as chaves `aggregate.*` ficaram sem uso após a remoção dos widgets (inofensivo; poda fora de escopo).
- **Barrel:** `apps/dashboard/src/features/workspace/index.ts` (re-export dos 4 componentes novos) foi adicionado além do censo declarado — adição de convenção, em escopo no subprojeto.
- **Artefato residual:** `apps/dashboard/src-tauri/target-qa/` (build isolado do reviewer) permanece — gitignored (`target*`), inofensivo.
- **Boundary warning desatribuído:** toda edição disparou um aviso citando spec não-relacionada (`suporte-agnostico-flutter-dart-linkar`) — estado de sessão obsoleto, não violação (defeito conhecido do boundary-gate por mtime).