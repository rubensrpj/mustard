# Followup — economia-moat-unification: 4 fixes pós-CLOSE

## PRD

Após o CLOSE da feature parent, 4 gaps surfaceram em uso real do dashboard:

1. **Trace coexistiu com Timeline+Eventos** — W6 deveria SUBSTITUIR as duas tabs antigas, mas o agente decidiu coexistir interpretando "pode coexistir" da spec. Hoje a `SpecDrillDown` tem 7 tabs (`Ondas, Trace, Qualidade, Timeline, Eventos, Rede, Sub-specs`). Combinado: 5 (`Ondas, Trace, Qualidade, Rede, Sub-specs`). Trace absorve o que Timeline+Eventos mostravam (linear view como modo dentro do Trace, se necessário).

2. **Visual do Trace ≠ claude-devtools** — `ExecutionTrace.tsx` usa `<BaseRow>` flat com ícones de 13px, sem cards, sem badges semânticos por tipo de evento, sem `<DiffViewer>` inline com path como header. Comparado às imagens de referência (`Image 6` + `Image 7` da conversa): cards elevated, ícones grandes, headers de bloco com nome (`Tasks`/`ToolUseBlock`/`Edit`), hierarquia indentada com conector visual rico, diff split com syntax highlighting.

3. **Badge "execute" para tudo em PIPELINES ATIVOS** — `WorkspaceHero` (ou backend `dashboard_workspace_summary`) está exibindo phase em vez de status. Specs em `closed-followup`/`completed` aparecem como "execute" porque foi a última `pipeline.phase` event registrada. Inconsistência confirmada: card "SPECS POR ESTADO" mostra 11 concluídas (lê status); "PIPELINES ATIVOS" mostra todas execute (lê phase). Fix: cruzar com `pipeline.status` ou filtrar fora as terminais antes de listar como "ativo".

4. **Economia tudo zero** — sub-causa primária já mitigada (mustard-rt reinstalado em 2026-05-21T07:25). Falta validar: (a) próxima sessão real popula `savings_records` e `spans`? (b) `MUSTARD_TRANSCRIPT_WATCH=1` é opt-in — incluir nota no Settings ou Preferences? (c) backfill 1x dos transcripts existentes em `~/.claude/projects/` via `mustard-rt run transcript-watcher --once` (criar flag se não existe).

## Contexto

Spec parent (`2026-05-20-economia-moat-unification`) está em `closed-followup` (janela de 24h pra fixes pós-CLOSE). Esse sub-spec consome essa janela. Linka via `### Parent:` header para a tree do dashboard mostrar como child.

## Métrica de sucesso

- `SpecDrillDown` tem exatamente 5 tabs (sem Timeline nem Eventos).
- ExecutionTrace renderiza um spec real com ícones grandes coloridos, cards elevated, e diff inline para events `Edit`/`Write` — visual matches imagens 6 e 7 de referência.
- "PIPELINES ATIVOS" no Workspace mostra apenas specs com status `implementing`/`approved`/`draft`/`awaiting-qa`/`reviewing` — NÃO mostra `completed`/`cancelled`/`closed-followup`.
- Após 1 sessão Claude Code real pós-reinstall: `SELECT COUNT(*) FROM savings_records WHERE source='RtkRewrite'` > 0 (e idem para `spans`).

## Não-Objetivos

- Não redesenhar outras pages (Economia.tsx, Knowledge, etc.).
- Não rescrever DS primitivas (`<DiffViewer>` etc. ficam como estão; apenas USAR melhor no Trace redesign).
- Não migrar páginas legadas para i18n (ainda lazy).

## Acceptance Criteria

- [x] AC-1: Build do dashboard passa — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Type-check passa — Command: `pnpm --filter mustard-dashboard exec tsc --noEmit`
- [x] AC-3: `SpecDrillDown` tem 5 tabs (sem Timeline nem Eventos) — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');if(/Timeline/.test(t)||/\"Eventos\"/.test(t))throw new Error('Timeline/Eventos tab labels still present')"`
- [x] AC-4: ExecutionTrace usa cards elevated — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/trace/ExecutionTrace.tsx','utf8');if(!/ds-surface-elevated|rounded-\\[--ds-radius-md\\]/.test(t))throw new Error('cards/elevated styling missing')"`
- [x] AC-5: ToolEventRow renderiza header de bloco com path — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src/components/trace/ToolEventRow.tsx','utf8');if(!/file_path|node\\.label|payload\\.path/.test(t))throw new Error('path header missing')"`
- [x] AC-6: `dashboard_workspace_summary` filtra terminais de PIPELINES ATIVOS — Command: `node -e "const t=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');if(!/completed|closed.followup|cancelled/.test(t))throw new Error('terminal status filter missing')"`
- [x] AC-7: `mustard-rt run transcript-watcher --once` existe (backfill flag) — Command: `node -e "const t=require('fs').readFileSync('apps/rt/src/run/transcript_watcher.rs','utf8');if(!/--once|once/i.test(t))throw new Error('--once flag missing')"`
- [x] AC-8: cargo check rt + core passam — Command: `cargo check -p mustard-rt -p mustard-core`

## Plano

3 sub-tarefas independentes + 1 verificação:

### Tabs merge (~30 LOC)
`SpecDrillDown.tsx`: remover labels `Timeline` e `Eventos` do array `TABS`. Remover `useSpecTimeline`/`useSpecEvents` queries (não mais necessárias) OU mover dados pra dentro do `<ExecutionTrace>` se ele precisar do shape linear como fallback. Substituir TabsContent dos 2 removidos por nada (ou redirecionar handlers de `handleTimelineNodeClick` pra dentro do Trace tab). Eliminar comentário stale que justificava coexistência.

### Trace redesign (~150-200 LOC)
`ExecutionTrace.tsx` + `ToolEventRow.tsx`: reescrever visual seguindo claude-devtools:
- Cada nó vira card com `border border-[--ds-surface-elevated] rounded-[--ds-radius-md] bg-[--ds-surface-base]` quando expandido.
- Header de cada nó: ícone 18-20px colorido por kind (spec roxo, wave azul, agent verde, tool âmbar), título grande (`text-sm font-medium`), badges (kind name, model, duration, tokens) à direita.
- Hierarquia: indentação por padding-left no container do bloco interno; conector visual via barra vertical sólida `border-l-2` (não tracejada) na cor do parent.
- Tool nodes: header mostra nome do tool (`Edit`, `Read`, `Bash`) + file path quando aplicável; payload abaixo. `Edit`/`Write` → `<DiffViewer mode="split" before={...} after={...} />` direto, com path como subheader. `Read` → `<CodeBlock code={excerpt} lang={detectLang(path)} />`. `Bash` → `<CodeBlock code={cmd + '\n---\n' + stdout} lang="plain" />`.
- Lazy render mantido (browser `<details>`).
- Permitir colapsar tudo até nó raiz ("Collapse all" button no topo).

### Status badge fix (~30-50 LOC)
`apps/dashboard/src-tauri/src/spec_views.rs::dashboard_workspace_summary`: o campo retornado para "PIPELINES ATIVOS" deve filtrar status terminais (`completed`, `cancelled`, `closed-followup` — ou pelo menos `completed` + `cancelled` se closed-followup é considerado "ainda ativo"). Investigar a query (provavelmente já retorna o status; talvez o frontend que ignora). Se o backend já retorna status correto, ajustar `WorkspaceHero.tsx`/`PipelinesActive*` componente pra filtrar terminais.

### Economia data validation (próxima sessão)
Nada de código nesse sub-spec. Item de verificação manual:
- Rodar 1 sessão Claude Code real (1 comando bash via `rtk` para disparar `bash_guard` → `savings_records.source=RtkRewrite`).
- Conferir via dashboard `Economia.tsx` que cards mudaram de 0.
- Se ainda zero: investigar.

## Tarefas

### Dashboard Followup Agent

- [ ] **Tabs merge** — editar `SpecDrillDown.tsx`: tirar `Timeline` e `Eventos` do array `TABS` (linha 27). Remover `useSpecTimeline`/`useSpecEvents` calls + suas TabsContent. Remover `handleTimelineNodeClick`. Manter `eventsFilter` se outro lugar usar; senão eliminar. Update comment stale (linhas 23-26).
- [ ] **Trace redesign** — reescrever `ExecutionTrace.tsx` + `ToolEventRow.tsx` matching imagens 6 e 7 de referência. Use `<DiffViewer mode="split" />` em tool=Edit/Write. Use `<CodeBlock>` em tool=Read/Bash. Cards elevated (`border border-[--ds-surface-hover] rounded-[--ds-radius-md]`), ícones por kind 18-20px, hierarquia com `border-l-2` connector. Adicionar "Collapse all" / "Expand all" toggle.
- [ ] **Status badge fix** — investigar `spec_views.rs::dashboard_workspace_summary` (ou onde quer que `WorkspaceHero` consuma): garantir que specs com status `completed`/`cancelled` (ou `closed-followup` se for considerado terminal) NÃO aparecem no card "PIPELINES ATIVOS". O fix pode ser em (a) Rust filtrar antes de retornar, ou (b) TS filtrar antes de renderizar — escolher onde a fonte de verdade está.
- [ ] **Transcript watcher --once** — adicionar flag `--once` (ou subcommand `transcript-backfill`) em `apps/rt/src/run/transcript_watcher.rs` que escaneia `~/.claude/projects/<encoded-cwd>/*.jsonl` uma vez, ingest tudo via `sources::transcript::ingest` + writer, sai. Útil pra popular Economia sem precisar deixar o daemon rodando.
- [ ] Rodar `pnpm --filter mustard-dashboard build` + `cargo check -p mustard-rt -p mustard-core` — verde.

### Verificação manual (não-codigo)
- [ ] Após implementação: abrir dashboard, conferir que `SpecDrillDown` tem 5 tabs, Trace renderiza com visual rico, "PIPELINES ATIVOS" não lista specs concluídas, Economia mostra dados não-zero após `mustard-rt run transcript-watcher --once` rodar.

## Dependências

- Parent: [[2026-05-20-economia-moat-unification]] (closed-followup)
- W6 entregue (`ExecutionTrace`, `ToolEventRow`)
- W5 DS primitives (`DiffViewer`, `CodeBlock`, `TreeNode`, `MetricsPill`, `BaseRow`)
- W3b `transcript_watcher.rs` (ganha flag `--once`)
- `mustard-rt` reinstalado em 2026-05-21T07:25 (binário do PATH agora reflete fonte)

## Limites

Em escopo:
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx` (remover tabs)
- `apps/dashboard/src/components/trace/{ExecutionTrace,ToolEventRow}.tsx` (redesign visual)
- `apps/dashboard/src-tauri/src/spec_views.rs` (filtro status — apenas `dashboard_workspace_summary`)
- `apps/dashboard/src/components/workspace/WorkspaceHero.tsx` (alternativa: filtro no frontend)
- `apps/rt/src/run/transcript_watcher.rs` (flag `--once`)
- `apps/rt/src/run/mod.rs` (registrar `--once` se for novo subcommand)

Fora de escopo:
- Outras páginas do dashboard (Economia, Knowledge, etc.)
- DS primitives (apenas consumir, não modificar)
- Hooks W2 (já entregues e em uso pós-reinstall)
- Pages legadas pra i18n (lazy)
- Refactor de outras tabs (Ondas, Qualidade, Rede, Sub-specs ficam intactas)
