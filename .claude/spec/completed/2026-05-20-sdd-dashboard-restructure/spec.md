# Dashboard SDD-first: reorganização IA + drill-down por spec + correções operacionais

### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-20T13:00:00Z
### Lang: pt

## PRD

## Contexto

O dashboard atual fragmenta o ato central do Mustard — a **spec** — em três menus diferentes (Atividade, Telemetria, Qualidade) sem hierarquia clara. O usuário precisa saltar entre páginas para responder perguntas básicas ("o que está rodando? falhou em quê? quanto economizou?") e ainda assim sobra ambiguidade: a Telemetria mostra "Pipeline em andamento" (singular) quando há 17 specs em paralelo; "Atividade por hora" replica o contribution-graph do GitHub em escala errada (7×24h em vez de dias); "Agentes" lista apenas `subagent-tracker` em vez dos tipos reais (`Explore`, `general-purpose`, `Plan`, `dashboard-impl`); "Histórico" fica permanentemente vazio; o coletor OTEL aparece como "parado" sem indicar o porquê; e o card "Atividade por fase" ainda mostra `events.jsonl` no empty state apesar da migração para SQLite.

A raiz do problema não é cosmética. É **de modelagem**: o dashboard trata eventos como o objeto de primeira classe e specs como filtros opcionais. O usuário pensa o inverso — a spec é a unidade de trabalho; eventos, ondas, agentes, tokens, QA e critérios de aceitação (AC) são observabilidade sobre essa spec. Esta spec reverte essa direção: a **spec vira o objeto principal** da navegação, e três áreas core suportam: (1) uma visão geral do workspace, (2) a lista de specs (com drill-down ao clicar), e (3) economia/observabilidade global de tokens. Tudo mais sai do caminho.

## Usuários/Stakeholders

Mantenedores do Mustard (principal: Rubens). Indiretamente, qualquer usuário do `mustard-dashboard` que precisa entender o que está rodando, o que falhou, o que cada agente está fazendo e quanto está sendo economizado em tokens. A solicitação veio do Rubens em 2026-05-20 após sessão real de uso onde a Telemetria recém-redesenhada (`2026-05-19-telemetry-dashboard-redesign`) mostrou que as peças visuais funcionam mas a arquitetura de informação não entrega clareza.

## Métrica de sucesso

- **Coletor live de fato.** Badge "live" / "parado" reflete eventos realmente frescos no SQLite — quando o usuário roda uma sessão, o coletor mostra "live" em ≤5s.
- **Tokens com números reais.** A página Economia mostra valores diferentes de zero para RTK, hooks, routing e measured (Anthropic-measured via OTEL) quando há histórico real.
- **Agentes com tipo real.** O card "Agentes" lista `Explore`, `general-purpose`, `Plan`, `dashboard-impl`, `rt-impl`, etc. — não mais `subagent-tracker`.
- **Sidebar 3 entradas core.** Em vez de Atividade + Telemetria + Qualidade (3 menus sobrepostos), o sidebar terá: Visão Geral, Specs, Economia. Mais Knowledge + Configurações.
- **Spec como objeto principal.** Clicar numa spec abre drill-down inline com 4 abas: Ondas, Qualidade, Timeline, Eventos. Todos os dados antes espalhados em Telemetria + Qualidade + Atividade ficam contextualizados ali.
- **Ações por spec.** Cada spec card oferece menu com `Reabrir`, `Fechar`, `Remover` (com confirmação).
- **Zero sobreposição.** Auditoria final: nenhuma página mostra a mesma informação que outra. Eventos crus só na drill-down da spec (filtrados).
- **Visão Geral "Sala de Operações" (multi-track).** Metáfora: estúdio de gravação multi-track. Cada spec ativa é uma faixa horizontal mostrando posição real na pipeline (analyze→plan→execute→qa→close), com marcador de fase atual, label da onda em execução, e indicadores de estado (ativo / blocked / paused / closed). Comunica trabalho paralelo de relance — único entre dashboards de dev tools.
- **Brand visual único: mustard yellow.** Zero referências a indigo/violet/sky/emerald/etc. em CSS, Tailwind classes, ou design tokens. `--color-accent-mustard` é a única cor de marca. Status semantics em 3-color anchor: `--color-ok` (verde discreto), `--color-accent-mustard` (brand/attention), `--color-error` (vermelho discreto).
- **Auditoria Hallmark zero critical.** Cada página nova (Visão Geral, Specs, Economia) passa por hallmark audit em Wave 6 antes dos ACs. Meta: 0 critical findings.

## Não-Objetivos

- **Não trocar o OTEL collector** ou a forma como ele coleta spans/eventos. Esta spec lê dados existentes.
- **Não rewrite do schema SQLite.** Apenas novos índices se necessário para per-spec queries.
- **Não mudar identidade visual** — Linear+Notion dark + **mustard yellow accent** (`--color-accent-mustard`) + Inter ficam.
- **Não tocar `mustard-rt`** (pipeline runner). Esta spec é puramente do dashboard.
- **Não migrar TanStack Query**. Hooks continuam usando o padrão atual.
- **Não criar análises com IA / busca vetorial.** Filtros simples bastam.
- **Não preservar Atividade/Telemetria/Qualidade como páginas.** A consolidação é deliberada — manter as três seria a mitigação que esta spec rejeita.

## Critérios de Aceitação

Critérios binários, executáveis. Cada um roda da raiz do projeto; exit 0 = passou. Padrão `node -e "...includes()"` (cross-shell-safe per memory `feedback_ac_cross_shell_windows.md`).

- [x] AC-1: Dashboard build limpo — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-2: Workspace Rust compila — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-dashboard`
- [x] AC-3: Testes rt + dashboard backend passam — Command: `cargo test -p mustard-rt -p mustard-dashboard`
- [x] AC-4: Sidebar tem exatamente 5 entradas (Visão Geral, Specs, Economia, Knowledge, Configurações) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');for(const x of ['Visão Geral','Specs','Economia','Knowledge','Configurações']){if(!c.includes(x))process.exit(1)};for(const x of ['Atividade','Telemetria','Qualidade']){if(c.includes('>'+x+'<')||c.includes('label=\"'+x+'\"'))process.exit(2)}"`
- [x] AC-5: Página `Specs.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/pages/Specs.tsx'))process.exit(1)"`
- [x] AC-6: Componente `SpecCard.tsx` existe — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/specs/SpecCard.tsx'))process.exit(1)"`
- [x] AC-7: Componente `SpecDrillDown.tsx` existe com 4 abas (Ondas/Qualidade/Timeline/Eventos) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');for(const x of ['Ondas','Qualidade','Timeline','Eventos']){if(!c.includes(x))process.exit(1)}"`
- [x] AC-8: Componente `SpecTrackRow.tsx` existe (uma faixa por spec na Visão Geral) — Command: `node -e "if(!require('fs').existsSync('apps/dashboard/src/components/workspace/SpecTrackRow.tsx'))process.exit(1)"`
- [x] AC-9: 6 novos Tauri commands registrados em lib.rs — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');for(const t of ['dashboard_spec_card','dashboard_spec_waves','dashboard_spec_quality','dashboard_spec_timeline','dashboard_spec_events','dashboard_workspace_summary']){if(!c.includes(t))process.exit(1)}"`
- [x] AC-10: Tauri command de ações de spec registrado — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/lib.rs','utf8');if(!c.includes('dashboard_spec_action'))process.exit(1)"`
- [x] AC-11: Bug fix — `events.jsonl` removido do empty state de Telemetry-em-Specs e narrativa SQLite-first — Command: `node -e "const fs=require('fs'),p=require('path');function walk(d){if(!fs.existsSync(d))return [];let r=[];for(const f of fs.readdirSync(d,{withFileTypes:true})){const x=p.join(d,f.name);if(f.isDirectory())r=r.concat(walk(x));else if(f.name.endsWith('.tsx'))r.push(x)}return r}const c=walk('apps/dashboard/src').map(f=>fs.readFileSync(f,'utf8')).join('\n');if(c.includes('events.jsonl'))process.exit(1)"`
- [x] AC-12: Bug fix — `telemetry_agents` agrupa por payload subagent_type real (não actor source) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/telemetry_agg.rs','utf8');if(!c.includes('subagent_type')&&!c.includes('actor.type'))process.exit(1)"`
- [x] AC-13: Bug fix — paths em EffortPanel/Esforço não truncam no início (sem `...` prefix em arquivos do top) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/telemetry/EffortPanel.tsx','utf8');if(/direction:\s*['\\\"]?rtl/.test(c))process.exit(1)"`
- [x] AC-14: Página `Atividade.tsx` removida do sidebar (ou deletada se órfã) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Sidebar.tsx','utf8');if(c.includes('to=\"/activity\"')||c.includes(\"to='/activity'\"))process.exit(1)"`
- [x] AC-15: Páginas Workspace/Specs/Economia usam o page primitives barrel — Command: `node -e "const fs=require('fs');for(const p of ['Workspace','Specs','Economia']){const f='apps/dashboard/src/pages/'+p+'.tsx';if(!fs.existsSync(f))process.exit(1);const c=fs.readFileSync(f,'utf8');if(!c.includes('PageHeader')||!c.includes(\"@/components/page\"))process.exit(2)}"`
- [x] AC-16: Topbar LABELS map sincronizado com Sidebar (Visão Geral, Specs, Economia, Knowledge, Configurações) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/layout/Topbar.tsx','utf8');for(const x of ['Visão Geral','Specs','Economia']){if(!c.includes(x))process.exit(1)}"`
- [x] AC-17: Zero referências a cores indigo/violet/sky/emerald/amber/rose em `apps/dashboard/src/` (classes Tailwind) — Command: `node -e "const fs=require('fs'),p=require('path');function walk(d){let r=[];for(const f of fs.readdirSync(d,{withFileTypes:true})){const x=p.join(d,f.name);if(f.isDirectory()&&!['node_modules','dist','target'].includes(f.name))r=r.concat(walk(x));else if(/\.(tsx?|css)$/.test(f.name))r.push(x)}return r}const files=walk('apps/dashboard/src');const m=[];for(const f of files){const c=fs.readFileSync(f,'utf8');for(const color of ['indigo-','violet-','sky-','emerald-','amber-','rose-','slate-400','slate-500']){if(c.includes(color))m.push(f+':'+color)}}if(m.length){console.log('VIOLATIONS:',m.slice(0,10));process.exit(1)}"`
- [x] AC-18: Hallmark audit em Workspace.tsx retorna 0 critical findings — registrado em `.claude/.harness/audit-workspace.md` durante Wave 6 — Command: `node -e "const fs=require('fs');if(!fs.existsSync('.claude/.harness/audit-workspace.md'))process.exit(1);const c=fs.readFileSync('.claude/.harness/audit-workspace.md','utf8');if(/critical.*[1-9]/i.test(c))process.exit(2)"`
- [x] AC-19: Hallmark audit em Specs.tsx retorna 0 critical findings — Command: `node -e "const fs=require('fs');if(!fs.existsSync('.claude/.harness/audit-specs.md'))process.exit(1);const c=fs.readFileSync('.claude/.harness/audit-specs.md','utf8');if(/critical.*[1-9]/i.test(c))process.exit(2)"`

## Plano

## Informações da Entidade

Não cria entidade de domínio nova. Reaproveita projeções existentes do SQLite event store (`pipeline_state_for_spec`, `pipelines_from_db`) + cria novos shapes view-side:

| Shape | Campos | Origem |
|---|---|---|
| `SpecCard` | `{ spec, status, phase, scope, started_at, last_event_at, duration_ms, current_wave?, total_waves?, ac_passed, ac_total, files_touched, tools_used, model }` | `dashboard_spec_card(spec)` |
| `SpecWave` | `{ wave, role, status (queued/in_progress/completed/failed), started_at?, completed_at?, agent_type?, files_changed }` | `dashboard_spec_waves(spec)` |
| `SpecQualityItem` | `{ ac_id, ac_label, status (pass/fail/skip), wave?, command, last_run_at?, fail_reason? }` | `dashboard_spec_quality(spec)` |
| `SpecTimelineNode` | `{ ts, kind (phase/wave/qa/review/agent/tool), label, phase?, wave?, payload_summary }` | `dashboard_spec_timeline(spec)` |
| `SpecAction` | `{ action: "reopen" \| "close" \| "remove", spec, result: "ok" \| "error", message? }` | `dashboard_spec_action(spec, action)` |
| `WorkspaceSummary` | `{ events_per_minute, tokens_saved_today, specs_active_count, spec_tracks: SpecTrack[], alerts: WorkspaceAlert[], top_files_today: FileCount[] }` | `dashboard_workspace_summary(repo_path)` |
| `SpecTrack` | `{ spec, status, current_phase, current_wave?, total_waves?, agents_active, last_event_at, blocked_reason?, segments: PhaseSegment[] }` | parte de WorkspaceSummary |
| `PhaseSegment` | `{ phase: "analyze"\|"plan"\|"execute"\|"qa"\|"close", state: "completed"\|"active"\|"future" }` | parte de SpecTrack |
| `WorkspaceAlert` | `{ spec, kind: "blocked"\|"qa_fail"\|"build_broken"\|"review_rejected", message, ts }` | parte de WorkspaceSummary |

## Padrão de página obrigatório (referência: Knowledge.tsx)

**Toda página nova ou refatorada nesta spec deve seguir o mesmo padrão da página `Knowledge.tsx`.** Esse padrão já é regulado pelos guards em `apps/dashboard/CLAUDE.md` e pelas skills `dashboard-page-primitives` + `dashboard-use-queries-fanout`.

Esqueleto canônico:

```tsx
<div className="flex flex-col gap-6 w-full">
  <PageHeader breadcrumb={[...]} title="..." subtitle={...} description={<>...</>} />
  {/* search/filters opcionais */}
  {/* gate states em cascata: !projectsRoot → !activeWorkspaceId → !activeProject → carregando → dados */}
  <section className="flex flex-col gap-3">
    <SectionHeader title="..." right={...} />
    <DataCard padded>
      {/* conteúdo */}
    </DataCard>
  </section>
</div>
```

Primitivos compartilhados (em `apps/dashboard/src/components/page/`): `PageHeader`, `SectionHeader`, `EmptyState`, `DataCard`, `CollapsibleGroup`, `KPICard`, `PhaseChip`, `EventChip`, `WaveRowLabel`, `AcBreakdown`. Reaproveitar 100% — **não criar variantes**.

Guards específicos (já em `apps/dashboard/CLAUDE.md` — reforço aqui):

- `useQueries` da TanStack v5 para fan-out per-project. Key por `project.path`.
- `invoke()` SOMENTE em `src/lib/dashboard.ts` (ou `src/api/*.ts`). Páginas e componentes nunca chamam invoke direto.
- Null-guard `data?.field` em toda query.
- `HashRouter`, nunca `BrowserRouter`. **Adicionar/remover rota requer atualização TRÍPLICE: `App.tsx` + `Sidebar.tsx` + `Topbar.tsx` LABELS map.** (Memory `routing_implicit_boundary`.)
- Zustand via slices: `useStore((s) => s.field)`. Não destructure o store inteiro.
- `staleTime` setado em toda `useQuery`.
- `text-[13px]` é o tamanho padrão de body em páginas Knowledge-pattern; títulos via `PageHeader`/`SectionHeader`.

## Mapeamento dos cards atuais → novo destino

Toda crítica do usuário em 2026-05-20 ficou rastreada para um destino concreto. Esta tabela serve como prova de cobertura — se alguma linha não tiver destino, esta spec falhou.

| Card/elemento atual | Crítica do usuário | Onde resolve |
|---|---|---|
| `Em execução` (Telemetria > Atividade) | "Filtro de data não faz sentido aqui — é estado, não janela" | Removido. Substituído pela lista de Spec Tracks na Visão Geral (sem filtro de data — mostra estado atual). |
| `Pipeline em andamento` (singular) | "Se eu tiver 5 specs, qual aparece?" | Removido. Cada spec ativa vira uma faixa em Spec Tracks (todas visíveis em paralelo). |
| `Atividade por hora` (heatmap 7×24h) | "Qual benefício real?" | Removido. Substituído pelo live pulse "● {N} eventos/min" no topo da Visão Geral. |
| `Agentes` (subagent-tracker only) | "Não vi dados relevantes; já está em Esforço" | Bug fix Wave 1 (mostrar tipo real). Card movido para drill-down da Spec específica em `Specs > Ondas` (mostra qual agente rodou em qual onda). |
| `Eventos recentes` (5 últimos) | "Qual finalidade se temos aba Eventos?" | Removido. Acessível via `Specs > drill-down > Timeline/Eventos` filtrado pela spec. |
| `Histórico de pipelines` (sempre vazio) | "Está sempre vazio" | Bug investigado Wave 2 + dados ficam visíveis em `Specs` com filtro "encerradas". |
| `Critérios de aceitação` (sem agrupar) | "Não agrupado por spec, sem motivo de fail, sem onda" | `Specs > drill-down > Qualidade` — AC agrupados por wave com motivo de fail. |
| `Esforço no período` (paths `...rt\src\...`) | "Paths truncados no início" | Bug fix Wave 1 (middle-truncation + tooltip). Card permanece compacto no rodapé da Visão Geral. |
| `Atividade por fase` (empty state com jsonl) | "Ainda fala em events.jsonl" | Bug fix Wave 1 (narrativa SQLite). Card removido em Wave 6 — informação distribuída entre Spec Tracks (fase atual por spec) e Specs > drill-down > Timeline. |
| Sidebar `Atividade` | "Choca com Telemetria > Eventos" | Removido. `Atividade > Timeline` agrega informação irrelevante; `Atividade > Eventos` vira `Specs > drill-down > Eventos` filtrado. |
| Sidebar `Qualidade` | implícito | Consolidado em `Specs > drill-down > Qualidade`. |
| Sidebar `Telemetria` | implícito | Removido. Atividade vai para `Visão Geral` + `Specs`; Economia vira página própria. |

## Briefing de design (preserva identidade)

Identidade visual da spec anterior fica intacta: dark-first, **mustard yellow** (`--color-accent-mustard`) reservado para fase ativa / call-out, Inter, blocos Notion-like + listas Linear-like. Tabular-nums em todo número. Motion budget já estabelecido (`number-tick`, `wave-glow`, `once-on-mount-fade`) é reaproveitado — não adicionar novos primitives.

**Sidebar reorganizada** (5 entradas, ordem fixa):

```
Workspace: mustard
├── Visão Geral     ← novo Home (era placeholder)
├── Specs           ← consolida Atividade + Telemetria > Atividade + Qualidade
├── Economia        ← extrai Telemetria > Economia para página própria
├── Knowledge       ← inalterado
└── Configurações   ← inalterado
```

**Visão Geral** — "Sala de Operações" (multi-track):

```
MUSTARD                              ● 142 eventos/min
11 specs ativas         8.4M tokens economizados hoje
──────────────────────────────────────────

●  sdd-dashboard-restructure
   ─────●━━━━━━━━━━━━━━━━━━━━━            ▶
      ANALYZE         EXECUTE wave 2/6 • 3 agents

●  session-bound-amendments
   ───────────●━━━━━━━━━━━━━            ▶
                       EXECUTE wave 5/8 • 1 agent

○  telemetry-dashboard-redesign
   ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ ✓
                                                CLOSED

●  knowledge-base-port            ⚠ Wave 3 BLOCKED
   ───────────●━━━━                            │
                       EXECUTE wave 3/5 • paused
```

Anatomia da página (top → bottom):

1. **Status do Workshop (topo)**: pulse line "● {N} eventos/min" (live, número muda em real-time com `number-tick`) + count "X specs ativas" + hero "Y tokens economizados hoje" (peso heavy, tabular-nums).

2. **Spec Tracks (centro, protagonista)**: cada spec ativa OU recém-fechada (últimas 24h) é uma faixa horizontal:
   - **Marcador de estado** à esquerda: `●` (live, mustard yellow), `○` (closed, ink dim), `⚠` (blocked, error red), `⊘` (paused, muted)
   - **Nome da spec** + scope chip
   - **Track line** com 5 segmentos representando as 5 fases (analyze→plan→execute→qa→close):
     - Segmento concluído: linha cheia ink (`━`)
     - Segmento atual: marcador `●` mustard yellow na posição
     - Segmento futuro: linha tracejada muted (`─`)
   - **Label da onda atual** abaixo do marcador (ex: "EXECUTE wave 2/6 • 3 agents")
   - **Indicador direita**: `▶` (active), `⚠ BLOCKED` (alerta), `✓ CLOSED` (concluída)
   - **Click no track**: navega para `Specs` com essa spec expandida (drill-down)

3. **Alertas (lateral direita, ~280px)**: lista compacta de problemas (BLOCKED specs, QA fails recentes, builds quebrados). Cada item: spec + razão + timestamp. Click → drill-down naquela spec na aba Qualidade.

4. **Esforço compacto (rodapé)**: top 5 arquivos do dia com path completo + tooltip — sem truncar no início. Apenas como rodapé contextual; quem quer detalhe vai pra Specs > drill-down.

**Sem** chips de fase, **sem** contribution graph, **sem** heatmap de atividade horária. A spec é o foco; tudo orbita ela.

**Specs** (página core, lista + drill-down inline):
- **Top bar**: filtros (status: ativas | encerradas | todas; data: today / 7d / 30d / all) + busca por nome
- **Lista ordenada**: ativas primeiro (em ordem do plano de execução — ANALYZE → PLAN → EXECUTE → QA → CLOSE), depois encerradas em ordem reversa (mais recente primeiro)
- **Cada SpecCard** (collapsed):
  - Header: nome (truncate sem cortar prefixo) + status pill + fase atual + duração total
  - Mini-timeline horizontal (5 stations: analyze→plan→execute→qa→close) — reaproveita PipelineTimeline mas em densidade compacta
  - Quantitativos: waves completed/total + AC pass/total + files touched + tools used + model
  - Hover: kebab menu (`⋮`) → ações: Reabrir | Fechar | Remover
- **Click expande inline** → SpecDrillDown com 4 tabs:
  - **Ondas**: lista wave-by-wave (role, status, agent, files, started/completed)
  - **Qualidade**: AC agrupados por wave; cada AC mostra status (pass/fail/skip) + comando + motivo de fail
  - **Timeline**: cronológica de events da spec (phase changes, wave completions, qa results, review results) — clicável para filtrar Eventos
  - **Eventos**: stream cru filtrado pela spec, com busca + chips (tipo de evento, wave, agente)

**Economia** (página própria, igual ao que já temos na aba Economia atual):
- Reaproveita `EconomySection` existente
- Hero tokens economizados + 3 cards asimétricos (RTK 2fr / Hooks 1fr / Routing 1fr) + Prompt Economy (Cache hero + Contexto + Eventos) + Canary Tail collapsed
- Sem mudanças além de mover para rota própria

**Sumido completamente**:
- Atividade sidebar entry (Activity.tsx) — consolidada em Specs > drill-down > Eventos
- Telemetria sidebar entry (Telemetry.tsx) — Atividade dela vira Visão Geral + Specs; Economia dela vira página própria
- Qualidade sidebar entry (Quality.tsx) — consolidada em Specs > drill-down > Qualidade
- "Atividade por hora" heatmap em escala 7×24h — substituído pelo Contribution Graph em escala diária × 30d
- "Pipeline em andamento" (singular) — substituído pela lista de SpecCards
- "Eventos recentes" card — informação acessível via Specs > drill-down > Timeline

## Arquivos

### Wave 1 — Bug fixes operacionais + brand migration (mustard yellow único)

**Brand migration (indigo → mustard) — atômico, antes dos componentes novos:**

- Grep `apps/dashboard/src/` por todas as ocorrências de `indigo-*`, `violet-*`, `sky-*`, `emerald-*`, `amber-*`, `rose-*`, `slate-400`, `slate-500` em arquivos `.tsx`/`.css`. Liste cada uma.
- Mapear: cores de "brand/primary/attention" → `--color-accent-mustard`; cores de "ok/success" → `--color-ok`; cores de "error/danger" → `--color-error`; cinzas neutros → `text-muted-foreground`/`bg-muted`/`text-foreground`.
- Update `apps/dashboard/tailwind.config.*` (se existe) ou `apps/dashboard/src/style.css` (Tailwind 4 `@theme` block): trocar `primary` resolved color de indigo para `--color-accent-mustard`. Re-validar todos os `bg-primary`/`text-primary`/`border-primary` automaticamente herdam mustard.
- Validate visual: rodar dashboard, verificar todas as páginas existentes ainda fazem sentido (Knowledge não pode ficar incolor).

**Bug fixes:**

- `apps/dashboard/src-tauri/src/telemetry_agg.rs` (edição) — `telemetry_agents`: garantir agrupamento por `actor.type` REAL extraído do payload do evento `agent.start`. Hoje group-by-actor_id está OK no fold, mas se o evento traz `actor.type` igual ao hook source, o fix vai precisar de extrair de `payload.subagent_type` (do tool input). Confirmar com teste seed.
- `apps/dashboard/src-tauri/src/telemetry.rs` (edição surgical) — verificar `collector_health_from_freshness()`. Tornar log explícito em "parado" indicando última timestamp de evento.
- `apps/dashboard/src/components/telemetry/EffortPanel.tsx` (edição) — substituir truncate-from-start (provável `direction: rtl` ou `text-overflow` reverso) por **middle-truncation** (mostra início + … + fim) com tooltip mostrando path completo no hover.
- `apps/dashboard/src/pages/Telemetry.tsx` (edição) — remover string `events.jsonl` do empty state (Atividade por fase) e atualizar narrativa para "Os hooks gravam em .claude/.harness/mustard.db a cada uso de ferramenta."
- `apps/dashboard/.claude/hooks/subagent-tracker.js` (edição se necessário) — verificar se `subagentType` que vem de tool input está sendo emitido corretamente. Hoje o hook já parece correto; ajustar somente se a auditoria Wave 2 (telemetry_agg fix) descobrir que o evento ainda carrega "subagent-tracker" em vez do tipo real.

### Wave 2 — Backend per-spec rollups

- `apps/dashboard/src-tauri/src/spec_views.rs` (novo módulo) — 6 funções de agregação per-spec + 1 ação:
  - `spec_card(conn, spec) -> Result<SpecCard>` — un-único shape para a card
  - `spec_waves(conn, spec) -> Result<Vec<SpecWave>>` — folds `pipeline.wave.complete` events
  - `spec_quality(conn, spec) -> Result<Vec<SpecQualityItem>>` — folds `qa.result` events grouped by wave
  - `spec_timeline(conn, spec) -> Result<Vec<SpecTimelineNode>>` — chronological event nodes
  - `spec_events(conn, spec, filter) -> Result<Vec<TimelineEvent>>` — filtered raw stream
  - `spec_action(conn, spec, action) -> Result<SpecAction>` — reopen (`completed` → `active`), close (move dir + emit closed), remove (delete dir + emit removed)
  - `workspace_summary(conn, repo_path) -> Result<WorkspaceSummary>` — hero + chips + contribution cells (30d) + KPIs
- `apps/dashboard/src-tauri/src/lib.rs` (edição) — 7 Tauri commands wrappando spec_views: `dashboard_spec_card`, `dashboard_spec_waves`, `dashboard_spec_quality`, `dashboard_spec_timeline`, `dashboard_spec_events`, `dashboard_spec_action`, `dashboard_workspace_summary`.
- `apps/dashboard/src-tauri/tests/spec_views_test.rs` (novo) — 3-4 testes integração com DB seed.

### Wave 3 — Frontend primitives

- `apps/dashboard/src/lib/dashboard.ts` (edição) — typed wrappers para os 7 novos Tauri commands.
- `apps/dashboard/src/lib/types/specs.ts` (novo) — TS shapes: `SpecCard`, `SpecWave`, `SpecQualityItem`, `SpecTimelineNode`, `SpecAction`, `WorkspaceSummary`, `ContributionCell`.
- `apps/dashboard/src/hooks/useSpecCard.ts` (novo) — TanStack Query, `enabled: !!spec`.
- `apps/dashboard/src/hooks/useSpecWaves.ts` (novo)
- `apps/dashboard/src/hooks/useSpecQuality.ts` (novo)
- `apps/dashboard/src/hooks/useSpecTimeline.ts` (novo)
- `apps/dashboard/src/hooks/useSpecEvents.ts` (novo) — aceita filters
- `apps/dashboard/src/hooks/useWorkspaceSummary.ts` (novo)
- `apps/dashboard/src/hooks/useSpecAction.ts` (novo) — useMutation, invalida specs list
- `apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx` (novo) — topo: live pulse "● {N} eventos/min" (`number-tick` em update) + "X specs ativas" + hero "Y tokens economizados hoje"
- `apps/dashboard/src/components/workspace/SpecTrackRow.tsx` (novo) — UMA faixa horizontal por spec; marcador de estado + nome + track 5-segmentos + label da onda + indicador direita; click → navega para Specs > drill-down
- `apps/dashboard/src/components/workspace/SpecTracksList.tsx` (novo) — container; ordena por: ativas (mais recente atividade), recém-fechadas (≤24h), depois ocultas
- `apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx` (novo) — lateral direita ~280px; lista de problemas (blocked / qa fail / build broken / review rejected)
- `apps/dashboard/src/components/workspace/WorkspaceEffortFooter.tsx` (novo) — rodapé; top 5 files do dia com path COMPLETO + tooltip; sem truncar no início
- `apps/dashboard/src/components/specs/SpecCard.tsx` (novo) — collapsed view com header + mini-timeline + quantitativos + kebab menu
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx` (novo) — container com 4 tabs (Ondas/Qualidade/Timeline/Eventos)
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx` (novo)
- `apps/dashboard/src/components/specs/SpecQualityTab.tsx` (novo) — AC agrupados por wave, status + fail reason
- `apps/dashboard/src/components/specs/SpecTimelineTab.tsx` (novo) — timeline rica clicável
- `apps/dashboard/src/components/specs/SpecEventsTab.tsx` (novo) — stream filtrado por spec (reaproveita filtros do Activity.tsx)
- `apps/dashboard/src/components/specs/SpecActionMenu.tsx` (novo) — dropdown reopen/close/remove + ConfirmDialog para destructive
- `apps/dashboard/src/components/specs/SpecActionConfirm.tsx` (novo) — modal de confirmação para Remover

### Wave 4 — Page reorg

- `apps/dashboard/src/pages/Workspace.tsx` (rewrite de Home.tsx ou novo arquivo) — composição: WorkspaceHero + PhaseChips + ContributionGraph + WorkspaceKpiRow + Esforço (top files compactos com path completo)
- `apps/dashboard/src/pages/Specs.tsx` (novo) — TopBar (filtros + busca) + lista SpecCards + expand/collapse SpecDrillDown
- `apps/dashboard/src/pages/Economia.tsx` (novo) — embedded EconomySection (reaproveita componente Wave 3 da spec anterior)
- `apps/dashboard/src/components/layout/Sidebar.tsx` (edição grande) — remove entradas Atividade, Telemetria, Qualidade; adiciona Visão Geral, Specs, Economia (no topo); mantém Knowledge, Configurações
- `apps/dashboard/src/App.tsx` ou `src/router.tsx` (edição) — atualiza routing: remove /activity, /telemetry, /quality; adiciona /workspace (default), /specs, /economy

### Wave 5 — Spec actions backend + wiring

- `apps/dashboard/src-tauri/src/spec_views.rs` (edição) — implementar `spec_action` com 3 branches:
  - `reopen`: spec em `completed/` → mover de volta para `active/`, atualizar header `Status: implementing`, emitir `mustard-rt run emit-pipeline --kind pipeline.status --payload '{"status":"reopened"}'`
  - `close`: spec em `active/` → mover para `completed/`, atualizar header `Status: completed`, emitir `closed`
  - `remove`: deletar diretório da spec (active OU completed), emitir `pipeline.removed`. SOMENTE com confirmação.
- `apps/dashboard/src/components/specs/SpecActionMenu.tsx` (edição) — wire `useSpecAction` mutation, toasts de feedback
- `apps/dashboard/src/components/specs/SpecActionConfirm.tsx` (edição) — confirm dialog para Remover

### Wave 6 — Hallmark audit + page cleanup + a11y + ACs

**Hallmark audits (gate antes dos ACs):**

- Rodar `hallmark` skill em cada uma das 3 páginas novas: `Workspace.tsx`, `Specs.tsx`, `Economia.tsx`.
- Para cada audit: dispatch agente com skill `hallmark` + escopo da página. Output: findings (critical/major/minor) em formato markdown.
- Persistir cada audit em `.claude/.harness/audit-{page}.md` (consumido por AC-18/AC-19).
- Se algum critical: fixar e re-auditar até zero critical. Max 2 ciclos por página.

**Page cleanup + a11y:**

- Apagar arquivos órfãos:
  - `apps/dashboard/src/pages/Activity.tsx` (se não usado em outro lugar)
  - `apps/dashboard/src/pages/Telemetry.tsx` (se Wave 4 substituiu pelas 2 novas páginas)
  - `apps/dashboard/src/pages/Quality.tsx` (se consolidado em Specs)
- Migrar componentes ainda úteis para `apps/dashboard/src/components/telemetry/` → `components/specs/` ou `components/workspace/`
- a11y: focus-visible rings (`--color-accent-mustard`) em todos os botões/links interativos; SpecDrillDown tab navigation via Tab; ContributionGraph cells `role="img"` + `aria-label`
- `prefers-reduced-motion`: cortar `wave-glow` e `once-on-mount-fade`
- Final validate (rodando todos os ACs)

## Dependências

- **Spec ascendente:** `2026-05-19-telemetry-dashboard-redesign` (CLOSE 2026-05-20) — entregou componentes reaproveitados (PipelineTimeline, EffortPanel, EconomySection, hooks useTelemetry*). Esta spec **consolida e migra**, não duplica.
- **Não bloqueia:** outras specs do workspace. Esta é fully dashboard-scoped.

## Limites

- `apps/dashboard/src/` (rewrite + adições) — sidebar, pages, hooks, components workspace/specs
- `apps/dashboard/src-tauri/src/spec_views.rs` (novo módulo)
- `apps/dashboard/src-tauri/src/lib.rs` (registro de 7 commands)
- `apps/dashboard/src-tauri/src/telemetry_agg.rs` (edição Wave 1 — agent fix)
- `apps/dashboard/src-tauri/src/telemetry.rs` (edição surgical Wave 1 — collector log)
- `apps/dashboard/.claude/hooks/subagent-tracker.js` (edição condicional Wave 1)
- `apps/dashboard/src-tauri/tests/spec_views_test.rs` (novo)
- **Fora dos limites:**
  - `apps/rt/` (pipeline runner)
  - `packages/core/` (event store schema)
  - OTEL collector (coleta lado source)
  - identidade visual (cores, fonts, paleta base)
  - schema SQLite (apenas índices novos se preciso)
  - busca vetorial / IA pra resumir specs (fora)
