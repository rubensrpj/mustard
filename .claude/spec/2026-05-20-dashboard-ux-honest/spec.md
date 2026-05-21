# UX honesta do dashboard: classificação, cor por fase, markdown viewer e estados vazios

### Status: completed
### Phase: CLOSE
### Scope: full
### Checkpoint: 2026-05-20T22:30:00Z
### Lang: pt

## PRD

## Contexto

A entrega da camada SDD (waves 1-5 da auditoria 2026-05-20 + spec `sdd-domain-finalization`) consertou a fonte de dados: eventos têm atribuição correta, `mustard-specsdb` projeta status/fase tipados, materializer popula `specs`/`metrics_projection` e o dashboard delega aos adapters `*_v2`. Os dados agora fluem.

A UI ainda não reflete isso direito. Rodando o dashboard em 2026-05-20, o usuário identificou cinco buracos concretos:

1. **Visão Geral classifica errado.** A seção "Specs ativas" lista specs já terminadas (`session-bound-amendments`, `pipeline-state-from-sqlite` ambas com CLOSE). O Rust ordena ativas antes de terminadas mas não filtra — o dashboard renderiza a lista inteira sob o título "ativas".

2. **Indicador de fases é P&B sem cor por fase.** `SpecTrackRow.PhaseTrack` (linhas 67-104) usa caracteres ASCII (`━` / `─` / `●`) com classes Tailwind cinza-em-cinza. Apenas a fase active recebe `--color-accent-mustard`; completed e future são variações de `text-foreground` / `text-border`. Sem ícone, sem cor por fase, sem indicação de ondas concluídas/em execução/falhadas. O componente `PhaseStation` (com `Search` / `ClipboardList` / `Zap` / `CheckSquare` / `Archive` por fase) já existe em `apps/dashboard/src/components/telemetry/PhaseStation.tsx` — entregue pela spec `telemetry-dashboard-redesign` e nunca consumido pelo `SpecTrackRow`.

3. **Tokens economizados sempre `0`.** O shape JSON antigo (`WorkspaceSummary.tokens_saved_today: i64`) força `0` quando a projeção retorna `None`. O frontend renderiza "0 tokens" como se fosse zero economia real, em vez de "—" / "indisponível" como combinado na Wave 4.

4. **Alertas não agrupados.** `WorkspaceAlertsColumn` mostra uma row flat por evento — três falhas QA na mesma spec viram três rows. Sem agrupamento por spec/onda, a coluna fica saturada em sessões reais.

5. **"Eventos/min" não explica o que é.** Número grande no topo da Visão Geral sem tooltip — o usuário não sabe se é taxa instantânea, média diária, contagem global.

Na página Specs:

6. **Default abre em "Todas"** quando o caso de uso primário é "o que está rodando agora". Resultado: 73 specs scrollable em vez das 2-3 ativas que o usuário busca.

7. **Sem agrupamento por status.** Lista plana mistura ativas, encerradas, sem eventos, bloqueadas.

8. **Markdown viewer regrediu.** Versões anteriores deixavam o usuário abrir o `spec.md` / `wave-N/spec.md` / `qa-report.md` / `review-report.md`. O Tauri command `dashboard_spec_markdown` existe (`lib.rs`), mas a UI atual não tem botão pra invocá-lo.

9. **Drill-down tabs (Ondas / Qualidade / Timeline / Eventos) são pobres.** Mostram listas com pouco contexto: onda passou? deu certo? o que falhou? — informação que está nos dados (`WaveStatus`, `AcStatus`, `fail_reason`) mas não é destacada.

Página Economia:

10. **Estática quando RTK indisponível.** O processo Tauri não herda PATH do usuário no Windows; `rtk gain` falha; `telemetry.rs::rtk_summary` retorna `available: false`; UI renderiza zeros sem CTA. Outros canais de economia (measured/OTEL, prompt economy, routing) ficam ofuscados pelo bloco RTK vazio.

Página Conhecimento:

11. **Sem hierarquia visual.** Tipos diferentes de conhecimento (pattern, decision, lesson, friction) renderizam idênticos — mesma fonte, mesma cor, sem badge. Ruído alto, sinal baixo.

Solicitado por Rubens em 2026-05-20 logo após a entrega da auditoria. Esta spec **completa** o trabalho de UX honesta — fecha o ciclo "domínio correto → UI correta".

## Usuários/Stakeholders

Mantenedores do Mustard que usam o dashboard como interface SDD primária. O feedback veio do Rubens rodando o dashboard em sessão real após as 5 waves de domínio. Indiretamente, qualquer usuário que vai abrir a Visão Geral esperando ver o que está rodando (não o histórico todo) e que precisa decidir em < 5 segundos se há trabalho ativo ou problema.

## Métrica de sucesso

- **Visão Geral** mostra **apenas specs ativas** (status `is_active()` no Rust) na seção "Em execução"; specs recentes encerradas vão para uma segunda seção "Concluídas hoje" colapsável.
- **`SpecTrackRow`** renderiza fases como `PhaseStation` (ícone + cor por fase + estado), não mais como caracteres ASCII. Visualmente igual ao protagonista da `PipelineTimeline`, em densidade compacta.
- **Tokens economizados** na Visão Geral mostra "—" quando indisponível, número formatado quando há dados. O shape JSON expõe `Option<i64>`.
- **Alertas** são agrupados por spec; dentro do grupo, alertas por kind/onda. Spec com 3 QA fails na mesma onda vira 1 row resumindo.
- **`events_per_minute`** ganha tooltip explicando: "taxa de eventos do harness na última janela de 60 segundos".
- **Página Specs** abre por default com filtro `Ativas`, e quando o usuário pede "Todas" os grupos aparecem ordenados por status (Ativas → Em revisão → Bloqueadas → Concluídas → Sem eventos).
- **Markdown viewer** restaurado: ícone na `SpecCard` e na drill-down aba `Timeline` abre modal/painel renderizando `spec.md` / wave `spec.md` / `qa-report.md` / `review-report.md` quando existirem.
- **Drill-down tabs** mostram explicitamente: por onda (passou/falhou + duração + último erro); por AC (status + comando + stderr quando falha); por timeline (evento + impacto + linkagem cruzada).
- **Página Economia** com RTK indisponível mostra empty state contextual ("RTK não está no PATH do dashboard — instale ou configure") **antes** das outras seções, e os canais measured/prompt-economy/routing renderizam normalmente abaixo.
- **Página Conhecimento** ganha cor + badge por tipo (`pattern`, `decision`, `lesson`, `friction`); hierarquia visual reduz ruído.
- Toda mudança preserva a paleta mustard yellow (`--primary` / `--color-accent-mustard`) — zero `indigo`/`violet`/`sky`/`emerald`/`amber`/`rose`.
- Hallmark audit em `Workspace.tsx`, `Specs.tsx`, `Economia.tsx` e `Knowledge.tsx` retorna 0 critical findings.

## Não-Objetivos

- **Não reescrever `mustard-specsdb`.** O domínio está fechado; esta spec consome o que existe (memória `feedback_dashboard_value_over_features` aplicada).
- **Não recriar `Telemetry.tsx` como rota separada.** A consolidação em Visão Geral + Specs fica.
- **Não instrumentar emissão automática de `rtk.savings` events.** RTK continua sendo processo externo; o dashboard apenas comunica indisponibilidade quando falta.
- **Não reverter mustard yellow** para violet. A migração de cor da Wave 1 fica intocada.
- **Não introduzir banner de migração** para shape antigo do `WorkspaceSummary` (memória `feedback_no_migration_dev_phase`). Mudar `tokens_saved_today` de `i64` para `Option<i64>` é uma quebra explícita; o frontend é o único consumidor e é atualizado no mesmo PR.
- **Não fazer QA visual automatizado via Playwright** (mesma decisão da `sdd-domain-finalization`). Visual QA fica na Checklist como item manual.
- **Não criar markdown editor.** Só viewer (`react-markdown` já está nas deps do dashboard per memória `react_markdown_v10`).

## Critérios de Aceitação

Critérios binários, executáveis. `node -e "...includes()"` cross-shell (memória `feedback_ac_cross_shell_windows`).

- [x] AC-1: Workspace inteiro compila — Command: `cargo build --workspace`
- [x] AC-2: Workspace passa testes — Command: `cargo test --workspace --exclude mustard-dashboard`
- [x] AC-3: Dashboard frontend compila — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-4: Dashboard backend testes passam — Command: `cargo test -p mustard-dashboard`
- [x] AC-5: `WorkspaceSummary.tokens_saved_today` no shape Rust legacy do dashboard é `Option<i64>` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src-tauri/src/spec_views.rs','utf8');const m=c.match(/pub struct WorkspaceSummary\s*\{[^}]*\}/s);if(!m||!/tokens_saved_today:\s*Option<i64>/.test(m[0]))process.exit(1)"`
- [x] AC-6: `SpecTrackRow.tsx` importa `PhaseStation` e não usa mais caracteres ASCII `━`/`─` para o fase track — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/workspace/SpecTrackRow.tsx','utf8');if(!c.includes('PhaseStation'))process.exit(1);const t=c.indexOf('function PhaseTrack');const e=t>=0?c.indexOf('export function SpecTrackRow',t):-1;const fn=t>=0?c.slice(t,e>=0?e:c.length):'';if(fn.includes('━')||fn.includes('─'))process.exit(2)"`
- [x] AC-7: `WorkspaceAlertsColumn.tsx` agrupa alertas por `spec` (busca por `groupBy` ou `Map` por spec) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx','utf8');process.exit(/(groupBy|new Map\(\)[\s\S]*alert\.spec|reduce[\s\S]*alert\.spec)/.test(c)?0:1)"`
- [x] AC-8: `WorkspaceStatusBar.tsx` tem tooltip/`title` explicando `events_per_minute` (string contendo "60 segundos" ou "60s") — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx','utf8');process.exit(/(title=[^>]*60\s*(s|segundos)|aria-label=[^>]*60\s*(s|segundos))/i.test(c)?0:1)"`
- [x] AC-9: `Specs.tsx` default `statusFilter` é `"ativas"` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Specs.tsx','utf8');process.exit(/useState<StatusFilter>\(\s*['\"]ativas['\"]/.test(c)?0:1)"`
- [x] AC-10: `Specs.tsx` agrupa por status quando filtro é "todas" (busca por `groupedByStatus` ou renderização condicional de cabeçalho de grupo) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Specs.tsx','utf8');process.exit(/(groupedByStatus|GROUP_ORDER|groupBy\(.*status\))/i.test(c)?0:1)"`
- [x] AC-11: Componente `SpecMarkdownViewer.tsx` existe e é importado em `SpecCard.tsx` ou `SpecDrillDown.tsx` — Command: `node -e "const fs=require('fs');if(!fs.existsSync('apps/dashboard/src/components/specs/SpecMarkdownViewer.tsx'))process.exit(1);const a=fs.readFileSync('apps/dashboard/src/components/specs/SpecCard.tsx','utf8');const b=fs.readFileSync('apps/dashboard/src/components/specs/SpecDrillDown.tsx','utf8');process.exit((a.includes('SpecMarkdownViewer')||b.includes('SpecMarkdownViewer'))?0:1)"`
- [x] AC-12: `SpecWavesTab.tsx` mostra status colorido por onda + duração (busca por `WaveStatus` import e `formatDuration` uso) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecWavesTab.tsx','utf8');process.exit((c.includes('WaveStatus')||c.includes('wave.status'))&&/(formatDuration|duration_ms)/.test(c)?0:1)"`
- [x] AC-13: `SpecQualityTab.tsx` mostra `fail_reason` quando AC falhou — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/components/specs/SpecQualityTab.tsx','utf8');process.exit(c.includes('fail_reason')?0:1)"`
- [x] AC-14: `Economia.tsx` mostra empty state explicativo quando `telemetry.data?.rtk?.available === false` — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');process.exit(/(available\s*===?\s*false|rtkUnavailable|RTK[\s\S]*indispon)/i.test(c)?0:1)"`
- [x] AC-15: `Knowledge.tsx` renderiza badge tipado por entry kind (busca por mapa de cores por type) — Command: `node -e "const c=require('fs').readFileSync('apps/dashboard/src/pages/Knowledge.tsx','utf8');process.exit(/(KIND_BADGE|TYPE_COLOR|typeColor|kindBadge|kindColor)/i.test(c)?0:1)"`
- [x] AC-16: Zero classes Tailwind `indigo-`/`violet-`/`sky-`/`emerald-`/`amber-`/`rose-` nos arquivos modificados — Command: `node -e "const fs=require('fs');const files=['apps/dashboard/src/components/workspace/SpecTrackRow.tsx','apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx','apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx','apps/dashboard/src/pages/Specs.tsx','apps/dashboard/src/pages/Economia.tsx','apps/dashboard/src/pages/Knowledge.tsx'];for(const f of files){if(!fs.existsSync(f))continue;const c=fs.readFileSync(f,'utf8');for(const color of ['indigo-','violet-','sky-','emerald-','amber-','rose-']){if(c.includes(color)){console.log('VIOLATION',f,color);process.exit(1)}}}"`
- [x] AC-17: Hallmark audit em Workspace + Specs + Economia + Knowledge registrado em `.claude/.harness/audit-ux-honest.md` com 0 critical — Command: `node -e "const fs=require('fs');if(!fs.existsSync('.claude/.harness/audit-ux-honest.md'))process.exit(1);const c=fs.readFileSync('.claude/.harness/audit-ux-honest.md','utf8');if(/critical.*[1-9]/i.test(c))process.exit(2)"`

## Plano

## Informações da Entidade

Sem entidade nova. Esta spec só consome:

- `mustard_specsdb::{SpecReader, SpecStatus, WaveStatus, AcStatus, SpecView, ...}` — domínio entregue na auditoria 2026-05-20.
- Tauri commands existentes (`dashboard_spec_card`, `dashboard_spec_waves`, `dashboard_spec_quality`, `dashboard_spec_timeline`, `dashboard_workspace_summary`, `dashboard_spec_markdown`) e seus adapters `*_v2`.
- Componentes existentes em `apps/dashboard/src/components/telemetry/{PhaseStation, PipelineTimeline, EffortHeatmap}.tsx`.
- `react-markdown` (já dep do dashboard, memória `react_markdown_v10`).

Um único shape muda: o Rust legacy `WorkspaceSummary` no `apps/dashboard/src-tauri/src/spec_views.rs` passa `tokens_saved_today` de `i64` para `Option<i64>` (alinhando com `mustard_specsdb::WorkspaceSummary`). Frontend é atualizado no mesmo PR.

## Arquivos

```
# Wave 1 — Visão Geral honesta
apps/dashboard/src-tauri/src/spec_views.rs                     — WorkspaceSummary.tokens_saved_today: Option<i64>; mapper preserva None
apps/dashboard/src-tauri/src/lib.rs                            — fallback do dashboard_workspace_summary já carrega Option
apps/dashboard/src/lib/types/specs.ts                          — TS WorkspaceSummary.tokens_saved_today: number | null
apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx — tooltip events/min; render "—" para tokens_saved_today null
apps/dashboard/src/components/workspace/SpecTrackRow.tsx       — substituir PhaseTrack ASCII por compact PhaseStation row
apps/dashboard/src/components/workspace/SpecTracksList.tsx     — split em "Em execução" + "Concluídas hoje" (collapsible)
apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx — agrupar por spec; resumo por kind/wave

# Wave 2 — Specs UX honesta
apps/dashboard/src/pages/Specs.tsx                             — default statusFilter "ativas"; agrupamento por status quando "todas"
apps/dashboard/src/components/specs/SpecCard.tsx               — ícone para abrir markdown viewer
apps/dashboard/src/components/specs/SpecMarkdownViewer.tsx     — novo: modal renderiza spec.md / wave/spec.md / qa.md / review.md
apps/dashboard/src/components/specs/SpecWavesTab.tsx           — destaca status por onda (verde/mustard/vermelho) + duração + último erro
apps/dashboard/src/components/specs/SpecQualityTab.tsx         — mostra fail_reason quando AC falhou (collapsible)
apps/dashboard/src/components/specs/SpecTimelineTab.tsx        — agrupa eventos por fase com badges

# Wave 3 — Economia funcional
apps/dashboard/src/pages/Economia.tsx                          — empty state contextual quando RTK indisponível
apps/dashboard/src/components/telemetry/EconomyRtkBlock.tsx    — novo (ou extrair): bloco RTK isolado com CTA quando ausente

# Wave 4 — Conhecimento formatado
apps/dashboard/src/pages/Knowledge.tsx                         — badge tipado, cor por kind, hierarquia
apps/dashboard/src/components/knowledge/KnowledgeBadge.tsx     — novo: badge reutilizável (pattern/decision/lesson/friction)

# Wave 5 — Auditoria
.claude/.harness/audit-ux-honest.md                            — gerado pela skill hallmark
```

## Tarefas

### Wave 1 — Visão Geral honesta (rt/dashboard)

- [x] `apps/dashboard/src-tauri/src/spec_views.rs`: trocar `tokens_saved_today: i64` por `Option<i64>` na struct `WorkspaceSummary`; mapper `workspace_summary_from_view` preserva `None` em vez de `unwrap_or(0)`.
- [x] `apps/dashboard/src-tauri/src/lib.rs`: fallback de `dashboard_workspace_summary` mantém `tokens_saved_today: None`.
- [x] `apps/dashboard/src/lib/types/specs.ts`: `tokens_saved_today: number | null` (era `number`).
- [x] `apps/dashboard/src/components/workspace/WorkspaceStatusBar.tsx`: renderizar "—" quando `tokens_saved_today === null`; adicionar `title` no número de events/min explicando "taxa na última janela de 60 segundos".
- [x] `apps/dashboard/src/components/workspace/SpecTrackRow.tsx`: substituir `PhaseTrack` ASCII por uma linha compacta de `PhaseStation` (ícone + cor + estado), reaproveitando o componente existente. Densidade reduzida (sem labels longos), apenas o ícone colorido + estado.
- [x] `apps/dashboard/src/components/workspace/SpecTracksList.tsx`: dividir em duas seções: "Em execução" (status `is_active()` no client, mesmo `TERMINAL_STATUSES` da Wave 5 da auditoria) e "Concluídas hoje" (status terminal + `last_event_at >= start of day`). Segunda seção colapsável, ocultada quando vazia.
- [x] `apps/dashboard/src/components/workspace/WorkspaceAlertsColumn.tsx`: agrupar alertas por `spec` via `Map<string, WorkspaceAlert[]>`. Cabeçalho do grupo: nome da spec; rows: kind + wave + ts. Quando `>3 alertas` na mesma spec, colapsar com link "ver todos".
- [x] `pnpm --filter mustard-dashboard build` + visualmente verificar no `tauri:dev`.

### Wave 2 — Specs UX honesta

- [x] `Specs.tsx`: `useState<StatusFilter>("ativas")` (era `"todas"`). Atualizar default chip selecionado.
- [x] `Specs.tsx`: quando `statusFilter === "todas"`, renderizar a lista agrupada por status: Ativas → Em revisão → Bloqueadas → Concluídas → Sem eventos. Cabeçalho de grupo: ícone + label + contagem.
- [x] Criar `apps/dashboard/src/components/specs/SpecMarkdownViewer.tsx`: modal full-screen com `react-markdown`, navegação por tabs (Spec / Onda N / QA / Review) consumindo `dashboard_spec_markdown` Tauri command. Param `{spec, kind, wave?}`.
- [x] `SpecCard.tsx`: ícone (Eye ou FileText) no header que abre `SpecMarkdownViewer` para essa spec.
- [x] `SpecDrillDown.tsx`: na aba Timeline e Ondas, links para "ver markdown" em cada item linkável (qa-report, review-report, wave-N).
- [x] `SpecWavesTab.tsx`: cada wave row destaca status colorido (verde `--color-ok` completed, mustard active, vermelho `--color-error` failed, neutro queued) + `formatDuration(duration_ms)` + se `failed` mostrar último erro abaixo.
- [x] `SpecQualityTab.tsx`: cada AC mostra status colorido + comando em monospace + se `fail_reason` presente, collapsible com stderr.
- [x] `SpecTimelineTab.tsx`: agrupa eventos por fase (collapsible groups por fase), badge por kind.
- [x] `pnpm --filter mustard-dashboard build`.

### Wave 3 — Economia funcional

- [x] `Economia.tsx`: detectar `telemetry.data?.rtk?.available === false`. Quando true, renderizar uma row dedicada no topo: ícone + "RTK não está disponível neste sistema" + texto curto explicando + (opcional) link/CTA para instalação.
- [x] Garantir que os outros canais (measured, prevention, routing, phases, promptEconomy) renderizam normalmente mesmo com RTK ausente — não bloquear seções inteiras por causa do RTK.
- [x] Extrair `EconomyRtkBlock.tsx` se isso ajuda a isolar o estado vazio sem bagunçar `EconomySection`. (Decidido NÃO extrair — bloco vazio é uma única `EmptyState` no topo do `Economia.tsx`; extrair criaria wrapper de 1 componente sem reuso e adicionaria salto de arquivo.)
- [x] Garantir `refetchInterval: 30_000` em `telemetry`, `usePromptEconomy`, `useTelemetryPhases` — confirmar que cada um polls (Wave 5 da auditoria fez isso só pros spec hooks). (`usePromptEconomy` era 60s → 30s; `useTelemetryPhases` era 5s → 30s; `telemetry` em `Economia.tsx` já estava em 30s.)
- [x] `pnpm --filter mustard-dashboard build`. (Compila sem erros nos arquivos da Wave 3; falhas restantes vêm de `Knowledge.tsx` Wave 4.)

### Wave 4 — Conhecimento formatado

- [x] Criar `apps/dashboard/src/components/knowledge/KnowledgeBadge.tsx`: badge tipado com prop `kind: "pattern" | "decision" | "lesson" | "friction"`. Cores: pattern (neutro `text-muted-foreground` + bg `bg-muted`), decision (`--primary` / mustard text), lesson (`--color-ok` text), friction (`--color-error` text + bg leve).
- [x] `Knowledge.tsx`: usar `KnowledgeBadge` em cada row (real + legacyFriction). Tipos derivam de `KnowledgeBrowseRow.type` (presente no shape) ou do nome para friction.
- [x] Adicionar hierarquia visual: rows de friction vão para uma sub-seção visualmente distinta dentro do bloco existente (já segregado, só falta o styling).
- [x] `pnpm --filter mustard-dashboard build`.

### Wave 5 — Hallmark audit + visual QA

- [x] Rodar skill `hallmark` contra `Workspace.tsx`, `Specs.tsx`, `Economia.tsx`, `Knowledge.tsx`. Output consolidado em `.claude/.harness/audit-ux-honest.md`. Esperado: 0 critical em todas; corrigir até atingir.
- [x] Visual QA manual: `pnpm tauri:dev`, navegar pelas 4 páginas, capturar screenshots e anotar regressões em `.claude/.harness/visual-qa-ux-honest.md`.

## Dependências

- Auditoria 2026-05-20 (waves 1-5): provê `mustard-specsdb`, adapters `*_v2`, mustard yellow theme. Esta spec **assume** todo esse trabalho landed.
- `sdd-domain-finalization` (spec sibling, em paralelo): traz `qa.result` automático e restauração da Visão Geral com `PipelineTimeline` + `EffortHeatmap`. **Não bloqueia** esta spec — Wave 3 da sibling toca `Workspace.tsx` (hero + heatmap); aqui tocamos `SpecTrackRow` e `WorkspaceAlertsColumn`. Coordenação: se ambas mexerem em `Workspace.tsx`, esta espera aquela ou ambas convergem num único PR.
- Wave 1 (Visão Geral) → Wave 2 (Specs): independentes, podem rodar em paralelo.
- Wave 3 (Economia) e Wave 4 (Knowledge): independentes das anteriores.

## Limites

- `apps/dashboard/src-tauri/src/{spec_views.rs, lib.rs}` — apenas a mudança de shape `Option<i64>` e mapper
- `apps/dashboard/src/lib/types/specs.ts` — sincronização de tipos TS
- `apps/dashboard/src/components/workspace/*` — `SpecTrackRow`, `SpecTracksList`, `WorkspaceAlertsColumn`, `WorkspaceStatusBar`
- `apps/dashboard/src/components/specs/*` — `SpecCard`, `SpecMarkdownViewer` (novo), `SpecDrillDown`, `SpecWavesTab`, `SpecQualityTab`, `SpecTimelineTab`
- `apps/dashboard/src/components/knowledge/KnowledgeBadge.tsx` — novo
- `apps/dashboard/src/pages/{Specs, Economia, Knowledge}.tsx`
- `.claude/.harness/{audit-ux-honest.md, visual-qa-ux-honest.md}` — gerados

**Fora dos limites:**

- `mustard-core`, `mustard-specsdb`, `mustard-rt` (camada de domínio congelada)
- OTEL collector
- Routing / Sidebar / Topbar
- Identidade visual / paleta de cores (mustard yellow fica)
- Workspace.tsx hero (Wave 3 da `sdd-domain-finalization` cuida disso)
- Heurística de backfill de ACs históricos
- Markdown editor (só viewer)
- `Telemetry.tsx` (já retirada)

## Checklist

- [x] Wave 1 — Visão Geral honesta
- [x] Wave 2 — Specs UX (default ativas, agrupamento, markdown viewer, tabs ricas)
- [x] Wave 3 — Economia funcional (estado vazio RTK)
- [x] Wave 4 — Conhecimento formatado (badge tipado)
- [x] Wave 5 — Hallmark audit + visual QA
- [x] `cargo build --workspace` verde
- [x] `cargo test --workspace --exclude mustard-dashboard` verde
- [x] `pnpm --filter mustard-dashboard build` verde
- [x] AC-1 a AC-17 todos com `[x]`

## Concerns

- **PhaseStation compactado via seletores arbitrários do Tailwind.** Em `SpecTrackRow.PhaseTrack` o componente é encolhido com `[&_span]:hidden [&>div]:w-5 [&>div]:h-5 [&_svg]:w-3 [&_svg]:h-3` (esconde labels/duração/contagem e reduz o círculo). Funciona, mas acopla `SpecTrackRow` à estrutura interna do `PhaseStation`. Se o `PhaseStation` ganhar um prop `density` (compact/normal) numa próxima wave, esta classe vira `density="compact"` e fica mais limpa. Fora do escopo da Wave 1 (limites da spec proibem tocar em `components/telemetry/`).
- **`PhaseStation` ativa `animate-wave-glow` no mount.** Numa Visão Geral com várias specs ativas, o efeito pisca em todas ao mesmo tempo (problema cosmético, não funcional). Avaliar na Wave 5 da auditoria visual.
- **AC-8 sensível à literal.** A regex `(title|aria-label)=[^>]*60\s*(s|segundos)` exige que `60 segundos` apareça inline no atributo, não em const externa. O texto foi inlineado para garantir match — extrair para const novamente exige ajustar a regex do AC ou um workaround.
