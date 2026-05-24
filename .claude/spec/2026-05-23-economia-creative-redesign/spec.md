# Tactical Fix: Economia — redesign criativo + ajustes de dados

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

Feedback empírico do usuário em 6 pontos:

### Por agente (azul)
- A1. Nomes técnicos (`core-impl`, `general-purpose`, `rt-impl`) sem leitura humana
- A2. "Top 3" sem totalizador; user quer ver o resto agregado + total que confere com o Custo do Projeto

### Por sessão (rosa)
- B1. Segunda coluna (short id `1151015b`) é irrelevante — drop
- B2. Linha sem spec ("sem spec registrada" em itálico) confusa — torná-la chip explícito
- B3. Nomes da spec cortados com `…` mesmo havendo espaço — layout/grid desperdiça largura

### Custo estimado por spec/onda (vermelho)
- C1. Segunda spec (i18n-migration) mostra 2 execuções, 2.5k tokens, **custo `—`** — bug ou display: investigar e corrigir
- C2. Quando spec NÃO tem onda atribuída, em vez de sub-row "(sem onda atribuída)" → **badge inline** colorido na linha da spec ("sem onda" pill em âmbar/zinc)
- C3. Quando spec TEM ondas → quebrar como hoje (sub-rows)
- C4. **Ordenar tudo por data descendente** — specs novas no topo

### Global
- "Use frontend-design, seja criativo, página está confusa"

## Decisão de design

**A. Per-agent: humanize + total**

- Mapeamento de nomes em PT/EN via i18n: `core-impl` → "Core (biblioteca)" / "Core (library)"; `general-purpose` → "Geral"/"General"; `rt-impl` → "Runtime"; `dashboard-impl` → "Dashboard"; etc. Fallback: tecnical id quando não mapeado
- Mostrar top 3, mais 1 linha "Outros (N agentes)" agregando o resto, mais 1 linha "Total estimado" no rodapé com soma — comparar visualmente com Custo do Projeto medido (linha discreta abaixo do total: "≈ medido $78.48"). A diferença esperada é o gap estimado×medido (rate fallback)

**B. Per-session: limpar coluna inútil + corrigir layout**

- Drop da coluna short-id (mantém só data/hora · spec(s) · USD)
- Nome da spec: ocupar todo o espaço disponível com `truncate` só quando realmente passa do width container. Usar `flex-1 min-w-0` corretamente; tooltip mostra full nome
- Linhas sem spec: chip cinza `<sem spec>` ao lado do USD em vez de italic
- Aumentar visual breathing — espaçamento generoso, hierarchy clara

**C. Per spec/wave: dados corretos + badge inline + ordenação**

- Fix C1: investigar por que i18n-migration spec mostra `—`. Possibilidades:
  - Rows novas (criadas após backfill --force) com pricing fresh — checar `cost_usd_micros` real no DB; se 0/NULL, é falha de ingestão pós-backfill. Caso confirmado: rodar `mustard-rt run backfill-run-usage-cost --force` mais uma vez antes do dashboard reabrir
  - Display: re-verificar `costMicros > 0` no row component; expandir threshold se necessário (cost < 1000 micros = $0.001 também é informação válida, exibir 4 casas decimais)
- C2/C3: quando `unwavedBySpec.has(spec) === true && (wavesBySpec.get(spec) ?? []).length === 0`, mostrar badge inline `sem onda` (pílula âmbar) no NOME da spec, em vez de sub-row. Quando tem ondas, manter sub-rows atuais (e ainda mostrar badge `sem onda` ao lado da spec se houver tanto ondas atribuídas quanto não-atribuídas).
- C4: ordenar por `spec_id desc` (Mustard usa prefixo YYYY-MM-DD nos slugs — sort lexical funciona) ou, se quiser ser preciso, adicionar `last_started_at` no SpecCost e ordenar por isso (backend mudança aditiva pequena)

**Global: frontend-design**

- Use a skill `frontend-design`. Aplicar:
  - Hierarquia visual: títulos de seção com peso/tamanho discriminante; números grandes em fonte tabular
  - Spacing rhythm: gaps consistentes (4/6/8) entre seções; padding generoso dentro de cards
  - Aproveitar `<KPICard caption=>` (recém-criado) onde fizer sentido
  - Cores: usar accents primary/amber/emerald já existentes com intent; evitar paredes de texto cinza
  - Microinteractions sutis: hover discreto, transições; nada de slop AI
  - Considerar redistribuir a página em 2 colunas em large screens (KPIs em cima, tabela à esquerda, savings à direita) — só se melhorar leitura

**Estimated badge inline** — design proposal:
```
2026-05-21-tf-detail-uses-speccard  [estimado]  [sem onda]   104  167k  $94.07
  ↳ wave-1                                                    50   80k  $45.10
  ↳ wave-2                                                    54   87k  $48.97
```
A badge "[sem onda]" só aparece quando há rows com wave_id=NULL. Cor: âmbar suave (`bg-amber-500/10 border-amber-500/30 text-amber-500`).

## Arquivos

- `packages/core/src/economy/model.rs` — `SpecCost` += `last_started_at: Option<i64>` (epoch-ms), `#[serde(default)]`
- `packages/core/src/telemetry/reader.rs` — `runs_by_spec_scoped` precisa SELECT também `MAX(started_at)`; ajustar shape de retorno
- `packages/core/src/economy/reader.rs` — `per_spec_costs` popula `last_started_at`; ordenação por `last_started_at DESC` em vez de `cost DESC`
- `apps/dashboard/src/lib/types/economy.ts` — TS interface `SpecCost` += `last_started_at: number | null`
- `apps/dashboard/src/i18n.ts` — bloco `economy.agents.*` com humanização dos nomes; chaves `economy.byAgent.total`, `economy.byAgent.others`, `economy.estimated.noWaveBadge`, `economy.bySession.noSpecChip`
- `apps/dashboard/src/components/economy/PerAgentTable.tsx` — humanização via map + linha "Outros" + linha "Total estimado" + comparação com medido
- `apps/dashboard/src/pages/Economia.tsx` — redesign SessionRow (drop short_id col, ajustar grid, chip empty), redesign EstimatedBySpecWave (badge inline em vez de sub-row para sem-onda, ordenação por last_started_at desc)

## Tarefas

### Library Agent (core)

- [x] `economy/model.rs`: `SpecCost` += `last_started_at: Option<i64>`, default None, comentário "epoch-ms of MAX(started_at) for the spec — used by UI for descending sort"
- [x] `telemetry/reader.rs::runs_by_spec_scoped`: SELECT também `MAX(started_at) AS last_started_at`; estrutura de retorno acomoda
- [x] `economy/reader.rs::per_spec_costs`: popular `last_started_at`; mudar ORDER BY de `cost_usd_micros DESC` para `last_started_at DESC NULLS LAST` (manter cost-desc como tiebreaker se duas specs tiverem mesmo timestamp)
- [x] Atualizar tests que assumem ordenação por cost
- [x] `cargo build && cargo test -p mustard-core --lib`

### UI Agent (dashboard) — load frontend-design skill

- [x] `lib/types/economy.ts`: refletir `last_started_at`
- [x] `i18n.ts`: bloco `economy.agents.*` (`core-impl`, `general-purpose`, `rt-impl`, `dashboard-impl`, `core-explorer`, `rt-explorer`, `dashboard-explorer`) com nomes humanos em PT e EN; chaves `economy.byAgent.total`, `economy.byAgent.others_one`/`_other`, `economy.byAgent.matchMeasured`, `economy.estimated.noWaveBadge`, `economy.bySession.noSpecChip`
- [x] `PerAgentTable.tsx`: helper `humanizeAgent(id, t)` que tenta `t(\`economy.agents.${id}\`)` e cai pra `id` se chave ausente; tabela mostra top-3 + linha "Outros (N)" agregando o restante + linha "Total estimado" no footer; embaixo do total, linha discreta "≈ medido $X.XX" (lê `data.total_cost_usd_micros` do summary, comparar)
- [x] `Economia.tsx::SessionRow`: drop coluna short_id; reorganizar grid: `1fr (date col w-20) 1fr (spec chips, flex-1) auto (usd col)`; spec name agora ocupa o espaço, com `truncate` só se realmente passar; chip cinza `<sem spec>` quando vazio (não italic)
- [x] `Economia.tsx::EstimatedBySpecWave`:
  - Investigar por que i18n-migration spec mostra cost `—`. Read 2-3 row samples via diagnóstico (mustard-rt query helper ou rodar `cargo run -p mustard-rt -- run backfill-run-usage-cost --force` de novo). Se for genuinamente $0, ajustar `formatUsd` para mostrar 6 decimais quando < $0.0001 (vir "$0.000750" em vez de "—")
  - Substituir lógica de "(sem onda atribuída)" sub-row por badge inline na spec row (chip âmbar `sem onda` ao lado do nome)
  - Ordenação: `[...namedSpecs].sort((a,b) => (b.last_started_at ?? 0) - (a.last_started_at ?? 0))` (fallback para sort lexical inverso se ambos null)
- [x] Visual polish: aplicar `frontend-design` skill — hierarchy, spacing, microinteractions sutis. Documentar 1-2 decisões criativas no return.

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: build dashboard verde — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-3: SpecCost tem last_started_at — Command: `bash -c "grep -q 'last_started_at' packages/core/src/economy/model.rs && echo ok"`
- [x] AC-4: tabela agentes tem total — Command: `bash -c "grep -q 'economy.byAgent.total' apps/dashboard/src/i18n.ts && echo ok"`
- [x] AC-5: short_id da SessionRow removido — Command: `node -e "const s=require('fs').readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');process.exit(/sessionId.*slice\(0,\s*8\)/.test(s)?1:0)"`
- [x] AC-6: agent humanizer presente — Command: `bash -c "grep -q 'humanizeAgent\\|economy.agents' apps/dashboard/src/components/economy/PerAgentTable.tsx && echo ok"`

## Limites

- Não tocar usage_totals (medido permanece)
- Não criar novo Tauri command (campo entra em SpecCost existente)
- Não migrar páginas além de Economia
- Surgical em pricing — degenerate fix só se realmente houver bug no compute_cost_micros
