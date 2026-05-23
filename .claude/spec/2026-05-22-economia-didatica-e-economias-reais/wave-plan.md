# Plano de Waves — Economia didática + economias reais

### Stage: Plan
### Outcome: Active
### Flags:
### Scope: full (wave plan)
### Lang: pt
### Total waves: 3

## Contexto

A página de Economia deveria explicar, para um usuário final, quanto o projeto
custou e quanto a ferramenta economizou — com clareza. Em vez disso, ela despeja
jargão interno (nomes de campo como `economy_summary.top_agents_by_cost`,
`usage_totals.cost.usage`; termos como spans, frames, "Prevention breakdown"),
sem uma linha sequer dizendo o que cada card significa. Pior: as economias de RTK
e de injeção de recipe aparecem sempre zeradas porque os emissores **nunca foram
codados de forma contínua** — o RTK só é ingerido por comando manual e a injeção
não tem emissor nenhum. O resultado é uma tela confusa que ainda mostra números
falsamente zerados, minando a confiança no que é apresentado.

## Usuários/Stakeholders

Quem abre a Economia para entender custo e economia do projeto. Pedido do Rubens
após revisar a tela: "ficou horrível, sem didática; e as economias nunca calculam".

## Métrica de sucesso

Cada card da Economia tem título claro em PT e uma linha explicando o que é e por
que importa, sem nome de campo interno nem jargão. Há um card por sessão com
data/hora, custo medido e as spec(s) trabalhadas naquela sessão. As economias de
RTK e de injeção passam a mostrar valores reais (não-zero quando houve atividade),
a de injeção rotulada "estimado". Builds e testes verdes.

## Não-Objetivos

- **Não** dropar a dimensão `model` do `usage_totals` — após o filtro de métricas
  os registros já são mínimos (≈27), a dimensão `session` é desejada para o card
  por-sessão, e `model` ainda alimenta o by-model da Telemetria.
- **Não** atribuir custo MEDIDO por spec/onda — a Anthropic reporta custo por
  sessão; spec/onda só existem no custo ESTIMADO (`run_usage`), que é o que será
  exibido (rotulado).
- **Não** adicionar "status" à telemetria — status é estado de pipeline (eventos
  no `mustard.db`), não custo.
- **Não** implementar agora os emissores de bash_guard block e budget cut — gaps
  conhecidos, fora do foco (RTK + injeção são o pedido).

## Critérios de Aceitação

Testáveis, binários (passa/falha). Cada um executável e independente.

- [ ] AC-1: Build do workspace passa — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-dashboard`
- [ ] AC-2: Testes core+rt passam — Command: `bash -c "cargo test -p mustard-core && cargo test -p mustard-rt"`
- [ ] AC-3: Emissor de economia de injeção existe — Command: `bash -c "grep -rq 'RecipeInjection' apps/rt/src && echo ok"`
- [ ] AC-4: RTK contínuo ligado no session_cleanup — Command: `bash -c "grep -riq 'rtk' apps/rt/src/hooks/session_cleanup.rs && echo ok"`
- [ ] AC-5: Reader expõe custo por sessão — Command: `bash -c "grep -rq 'by_session' packages/core/src/economy/model.rs && echo ok"`
- [ ] AC-6: Economia sem nome de campo interno nos rótulos — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('apps/dashboard/src/pages/Economia.tsx','utf8');process.exit(/economy_summary\.|usage_totals\.cost|savings_breakdown\b/.test(s)?1:0)"`

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-library]] | library | — | core: reader por-sessão (custo `usage_totals` + data/hora `updated_at` + spec(s) via `run_usage`, por `session_id`); helper da métrica de injeção; expor savings limpo p/ UI |
| 2 | [[wave-2-general]] | general | [[1]] | rt: emissores que faltam — injeção de recipe (`record_savings(RecipeInjection)` com proxy "geração evitada") + RTK contínuo (`session_cleanup` ingere `rtk gain --json`) |
| 3 | [[wave-3-ui]] | ui | [[1]] | dashboard: Economia didática (título PT + 1 linha por card, sem campo/jargão), card por-sessão (data/hora+custo+specs), economias populadas (RTK + injeção "estimado") + custo por spec/onda estimado rotulado |

## Critique Coverage

| Item levantado | Categoria | Onde |
|---|---|---|
| "Dashboard horrível, sem didática, jargão/abreviações" | Coberto | Wave 3 — reescrita didática de todos os cards |
| "usage_totals — só somatório por metric? tantos registros?" | Não-Objetivo | Já mínimo pós-filtro (≈27); `model` mantido (by-model usa); `session` é desejada |
| "Sem projeto/spec/onda/status na telemetria" | Coberto + Não-Objetivo | spec/onda via `run_usage` exibido (W3); projeto=per-DB; status não é custo |
| "Consumo geral acumulado + por sessão c/ data/hora + specs" | Coberto | Wave 1 (reader) + Wave 3 (card) |
| "RTK economizou — está 0" | Coberto | Wave 2 — ingestão contínua de `rtk gain` |
| "Economia de injeção nunca calculada — 0" | Coberto | Wave 2 — emissor novo + proxy "geração evitada" (estimado) |
| bash_guard block / budget cut zerados | Não-Objetivo | Gaps conhecidos, fora do foco desta spec |
