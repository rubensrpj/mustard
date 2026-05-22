# Polimento das abas do dashboard

### Parent: [[2026-05-21-dashboard-spec-tabs]]
### Stage: Close
### Outcome: Active
### Flags: followup_open
### Scope: full (wave plan)
### Checkpoint: 2026-05-21T17:00:00Z
### Lang: pt
### Total waves: 4

## PRD

## Contexto

A spec parent `2026-05-21-dashboard-spec-tabs` entregou o sistema de abas + 5 sub-abas, mas usando em runtime apareceram 9 follow-ups (1 deles — o mecânico do close-gate — vira spec separado, sem parent). Três são bugs reais (lista vazia ao primeiro mount, ondas somem durante EXECUTE, trace não expande); três são reorganizações da aba Ondas (Onda #0 = parent, pin do drawer, sub-specs como filhos da onda em vez de aba separada); um é redesign do grafo da Rede como mapa mental (era pequeno e ilegível); dois são polimento visual (cores por fase + piscar na fase ativa, paleta consistente em todos os badges). A spec parent fica em `closed-followup` até este polimento fechar.

## Usuários/Stakeholders

O usuário que abriu o dashboard pela primeira vez depois do CLOSE da parent e observou os 9 itens em uso real (não em smoke test). Pedido tem peso de "feature não usável sem isso" para itens 1, 5, 6; peso de "feature confusa sem isso" para 3, 4, 7, 8; peso de "feature crua sem isso" para 9, 10.

## Métrica de sucesso

Abrir a rota `/specs` no primeiro mount já mostra a lista de cards do workspace ativo (sem aba vazia). Durante uma pipeline rodando, a aba Ondas mostra todas as ondas declaradas no `wave-plan.md` independente de quantos eventos já vieram. Aba Trace expande cada evento. Cada onda na aba Ondas pode mostrar suas sub-specs aninhadas; a aba "Sub-specs" some. Botão pin no drawer da onda fixa o conteúdo inline dentro do painel. Grafo da Rede ocupa o painel inteiro como mapa mental radial. PipelineTimeline tem cor distinta por fase e a fase ativa pulsa. Todos os badges do app têm cor com semântica.

## Não-Objetivos

- Spec separado: fix sistêmico do `/mustard:close` para detectar tactical-fix candidates e emitir `closed-followup` automaticamente (item 2 do follow-up). Sai daqui porque é fix do Mustard core (cli/rt), não do dashboard. Vira outro `/feature` quando necessário.
- Atalho de teclado para abas (Cmd+T, Cmd+W) — continua pra depois.
- Persistência de abas entre sessões — continua pra depois.
- Re-design da Trace além de fazer o expand funcionar (a paleta atual fica).
- Reescrever o `<SpecMarkdownViewer>` antigo (era modal); a Onda #0 reusará o componente que já existe + o `<Markdown>` renderer.

## Critérios de Aceitação

Testable, binary (pass/fail) criteria. Each MUST be executable and independent.

- [x] AC-1: Workspace compila — Command: `cargo check --workspace`
- [x] AC-2: Testes do rt passam — Command: `cargo test -p mustard-rt --bin mustard-rt --no-fail-fast`
- [x] AC-3: Dashboard tipa e builda — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: SpecCard NÃO renderiza mais o ícone do markdown viewer (FileText) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');process.exit(/FileText/.test(s)?(console.error('FileText ainda presente'),1):0)"`
- [x] AC-5: SpecDrillDown NÃO inclui "Sub-specs" no array TABS — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');const m=s.match(/const\s+TABS\s*=\s*\[([\s\S]*?)\]\s*as\s+const/);process.exit(m && !/Sub-specs/.test(m[1])?0:(console.error('aba Sub-specs ainda no array TABS'),1))"`
- [x] AC-6: SpecWavesTab tem linha Onda #0 (parent spec ancorada) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecWavesTab.tsx','utf8');process.exit(/wave:\s*0\b|Onda\s*#?0/.test(s)?0:1)"`
- [x] AC-7: WaveMarkdownDrawer expõe modo `pinned` — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx','utf8');process.exit(/pinned\??:\s*boolean|onPinChange/.test(s)?0:1)"`
- [x] AC-8: PHASE_COLORS map existe (cores por fase) e animate-pulse aparece em PhaseStation/PipelineTimeline — Command: `node -e "const fs=require('fs');const p=fs.readFileSync('apps/dashboard/src/lib/phase-palette.ts','utf8');const ps=fs.readFileSync('apps/dashboard/src/components/telemetry/PhaseStation.tsx','utf8');const pt=fs.readFileSync('apps/dashboard/src/components/telemetry/PipelineTimeline.tsx','utf8');process.exit(/PHASE_COLORS/.test(p)&&(/animate-pulse/.test(ps)||/animate-pulse/.test(pt))?0:1)"`
- [x] AC-9: Rede SVG ocupa o painel (sem aspect-[4/3] fixo) — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecNetworkTab.tsx','utf8');process.exit(/aspect-\\[4\\/3\\]/.test(s)?(console.error('aspect-[4/3] ainda hard-coded'),1):0)"`

## Plano

## Informações da Entidade

Sem mudança de schema. Apenas reorganização do dashboard + uma pequena adição de query no rt para correlacionar sub-spec ↔ wave (campo `wave: Option<u32>` em `ChildEntry`, derivado de `started_at` da wave + timestamp do header do filho — heurística best-effort).

## Arquivos

```
apps/dashboard/src/pages/Specs.tsx                              — wave 1: empty state inicial
apps/dashboard/src/components/specs/SpecWavesTab.tsx            — wave 1+2: fallback FS + Onda #0 + sub-specs aninhadas + onda expandível
apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx      — wave 2: pin mode
apps/dashboard/src/components/specs/SpecCard.tsx                — wave 2: remove FileText viewer trigger
apps/dashboard/src/components/specs/SpecDrillDown.tsx           — wave 2: remove aba Sub-specs
apps/dashboard/src/components/specs/SpecDetailDashboard.tsx     — wave 2: ondas tab carrega children query
apps/dashboard/src/components/specs/SpecChildrenTab.tsx         — wave 2: deletado (ou reaproveitado pelo SpecWavesTab)
apps/dashboard/src/components/trace/ExecutionTrace.tsx          — wave 1: fix expand
apps/dashboard/src/components/trace/ToolEventRow.tsx            — wave 1: confirma expand
apps/dashboard/src/hooks/useSpecWaves.ts                        — wave 1: aceita fallback wave-plan FS
apps/dashboard/src/components/specs/SpecNetworkTab.tsx          — wave 3: mapa mental radial full-size
apps/dashboard/src/components/specs/spec-graph-layout.ts        — wave 3: layout radial
apps/dashboard/src/components/telemetry/PipelineTimeline.tsx    — wave 4: cores + pulse
apps/dashboard/src/components/specs/spec-status.tsx             — wave 4: paleta de badges
apps/dashboard/src/lib/dashboard.ts                             — wave 1: novo wrapper dashboardSpecWavesFromPlan (se necessário)
apps/dashboard/src-tauri/src/spec_views.rs                      — wave 1: command pra ler wave-plan + ondas declaradas
apps/rt/src/run/spec_children.rs                                — wave 2: incluir wave correlation
```

## Tarefas

Resumo de dependências:

```
wave-1 (bugs: tab vazia + ondas FS + trace expand)
                          │
                          ▼
wave-2 (Ondas restruct: #0 + pin + sub-specs por wave) ──┐
                                                          │
wave-3 (Rede mapa mental)                                 ├─►  review → qa
                                                          │
wave-4 (design: cores fase + badges)                     ─┘
```

Wave 1 corre primeiro (sozinha) porque os bugs bloqueiam o uso. W2-W4 podem rodar em paralelo (não há overlap forte: W2 toca SpecCard/SpecDrillDown/SpecWavesTab/SpecDetailDashboard/WaveMarkdownDrawer/spec_children.rs; W3 toca SpecNetworkTab/spec-graph-layout; W4 toca PipelineTimeline/spec-status). W2 e W3 podem rodar em paralelo (zero overlap). W4 também (toca arquivos distintos).

## Tabela de Waves

| Wave | Spec                | Role | Resumo                                                       |
|------|---------------------|------|--------------------------------------------------------------|
| 1    | [[wave-1-ui]]       | ui   | Bugs: aba lista vazia + ondas FS fallback + trace expand     |
| 2    | [[wave-2-ui]]       | ui   | Ondas restruct: #0 parent + pin + sub-specs aninhadas        |
| 3    | [[wave-3-ui]]       | ui   | Rede: mapa mental radial full-painel                          |
| 4    | [[wave-4-ui]]       | ui   | Design: cores por fase + pulse + paleta badges                |

## Dependências

- shadcn `Sheet` (já em uso).
- shadcn primitives + tailwind tokens já existentes (`--color-ok`, `--color-error`, `--color-accent-mustard`, etc.).
- `lucide-react` (Pin, PinOff icons já no pacote).
- Sem nova dep npm. Sem nova dep Cargo.

## Limites

- `apps/dashboard/src/pages/Specs.tsx`
- `apps/dashboard/src/components/specs/SpecCard.tsx`
- `apps/dashboard/src/components/specs/SpecDrillDown.tsx`
- `apps/dashboard/src/components/specs/SpecDetailDashboard.tsx`
- `apps/dashboard/src/components/specs/SpecWavesTab.tsx`
- `apps/dashboard/src/components/specs/SpecNetworkTab.tsx`
- `apps/dashboard/src/components/specs/SpecChildrenTab.tsx`
- `apps/dashboard/src/components/specs/WaveMarkdownDrawer.tsx`
- `apps/dashboard/src/components/specs/spec-graph-layout.ts`
- `apps/dashboard/src/components/specs/spec-status.tsx`
- `apps/dashboard/src/components/telemetry/PipelineTimeline.tsx`
- `apps/dashboard/src/components/trace/ExecutionTrace.tsx`
- `apps/dashboard/src/components/trace/ToolEventRow.tsx`
- `apps/dashboard/src/hooks/useSpecWaves.ts`
- `apps/dashboard/src/lib/dashboard.ts`
- `apps/dashboard/src-tauri/src/spec_views.rs`
- `apps/rt/src/run/spec_children.rs`

Out-of-boundary explicit: fix do `/mustard:close` para auto-detectar followup status (sai daqui), atalho de teclado, persistência de abas, redesign visual além das fases/badges.

## Cobertura de Críticas

| Crítica do usuário | Bucket | Onde |
|---|---|---|
| (1) Tab "Lista" vazia ao entrar | Coberto | Wave 1 |
| (2) Spec foi direto pra "concluída" em vez de "followup" | Não-Goal aqui (spec separado) | Não-Objetivos |
| (3) Remover botão de modal-da-spec da lista; Onda #0 = parent | Coberto | Wave 2 |
| (4) Janela direita com pin dentro da tab Ondas | Coberto | Wave 2 |
| (5) Ondas somem durante EXECUTE | Coberto | Wave 1 (FS fallback) |
| (6) Aba Trace não expande | Coberto | Wave 1 |
| (7) Grafo da Rede pequeno; mapa mental | Coberto | Wave 3 |
| (8) Sub-specs como subnível da onda | Coberto | Wave 2 (remove aba) |
| (9) Cores por fase + pulse na fase ativa | Coberto | Wave 4 |
| (10) Todos badges com cor | Coberto | Wave 4 |

Todos os 10 itens mapeados. Item (2) tem destino justificado (não-goal aqui, spec separado).
