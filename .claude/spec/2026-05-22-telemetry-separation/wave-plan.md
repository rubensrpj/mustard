# Plano de Waves — Separar e enxugar a telemetria

### Stage: Close
### Outcome: Completed
### Flags:
### Scope: full (wave plan)
### Lang: pt
### Total waves: 3

## Contexto

O banco do harness (`mustard.db`) é aberto pelos hooks a cada ação do usuário, então
seu tamanho penaliza toda a sessão. Hoje ele carrega ~42 MB de telemetria do
OpenTelemetry na tabela `claude_code_otel` — 62% do arquivo — gravada com
granularidade por minuto e dimensões que nenhuma tela consome. As consultas do
dashboard só extraem dela cinco números (custo total, custo por modelo, custo por
sessão, contagem de sessões, tempo ativo) e o instante mais recente para o selo de
frescor; minuto-a-minuto, `token_type`, `attrs`, `count` e `signal` nunca são
lidos. A telemetria de traces (`spans`, 2.3 MB) alimenta as outras medições do
dashboard (economia por spec/wave/agente, qualidade por fase, série diária, taxa de
cache). Hoje o coletor grava cada span SEM atribuição (`spec`/`wave`/agent ficam
nulos), e o reader de economia recupera essa atribuição por um JOIN de leitura com
os eventos `agent.start`. Esse acoplamento entre o log durável (`events`) e a
telemetria é um remendo: o certo é o span já nascer atribuído. O resultado atual é
um banco quente inflado por dado irrelevante e telemetria acoplada ao log de estado.

## Usuários/Stakeholders

Quem usa o dashboard (telas de economia/telemetria) e quem opera o Claude Code
com Mustard (cada ação espera os hooks abrirem o banco). Pedido do Rubens:
"toda telemetria separada e enxuta — só dado relevante; valor por projeto e total".

## Métrica de sucesso

O `mustard.db` deixa de conter qualquer telemetria; a telemetria vive em
`.harness/telemetry.db`, **totalmente independente** (zero JOIN/ATTACH com
`mustard.db`), e toda a lógica de telemetria mora num **módulo dedicado**
`packages/core/src/telemetry/` (SOLID: storage + escrita + leitura coesos,
trait-backed). As tabelas têm nomes claros: `usage_totals`, `run_usage`,
`run_attribution`. A `usage_totals` reduzida ocupa ordens de grandeza menos espaço
(sem minuto/`token_type`/`attrs`/`count`). Cada run nasce atribuído na escrita.
Todas as medições atuais do dashboard continuam com os mesmos valores. Builds e
testes verdes.

## Nomes das tabelas (telemetry.db)

| Nome | Conteúdo | Era |
|---|---|---|
| `usage_totals` | totais agregados de uso/custo (custo total, por modelo, por sessão, contagem de sessões, tempo ativo) | `claude_code_otel` |
| `run_usage` | uso/custo por execução (um run = rodada de agente/ferramenta): tokens, custo, duração, modelo, spec, wave, agent | `spans` |
| `run_attribution` | mapa `(session_id, tool_use_id) → (spec, wave, agent_id)` p/ carimbar o run na escrita | (novo) |

## Não-Objetivos

- **Não** alterar as medições mostradas no dashboard — preservar todas as
  mapeadas no inventário (custo total/modelo/sessão, sessão/tempo ativo, economia
  por spec/wave/agente, qualidade por fase, série diária, cache ratio).
- **Não** reduzir a tabela `spans` (já enxuta, 2.3 MB) nem suas colunas — só movê-la.
- **Não** mexer nos eventos efêmeros (`tool.result`/`tool.call`/`rtk-rewrite`) da
  tabela `events` — é outra frente, fora desta spec (telemetria OTEL).
- **Não** unificar `claude_code_otel` e `spans` — carregam dados de fontes
  diferentes (custo billing nativo vs. custo estimado por span).

## Critérios de Aceitação

Testáveis, binários (passa/falha). Cada um executável e independente.

- [x] AC-1: Build do workspace passa — Command: `cargo build -p mustard-core -p mustard-rt -p mustard-dashboard`
- [x] AC-2: Testes core+rt passam (verificado à mão: core 368, rt 789; qa-run dá skip por timeout de build a frio >120s) — Command: `bash -c "cargo test -p mustard-core && cargo test -p mustard-rt"`
- [x] AC-3: `usage_totals` sem colunas irrelevantes na DDL (ignora comentários) — Command: `node -e "const fs=require('fs');const raw=fs.readFileSync('packages/core/src/telemetry/schema.sql','utf8');const sql=raw.split('\n').filter(l=>!l.trim().startsWith('--')).join('\n');const i=sql.indexOf('usage_totals');const b=sql.slice(i,i+400);process.exit(/attrs|token_type|\bcount\b|signal/.test(b)?1:0)"`
- [x] AC-4: módulo dedicado de telemetria existe e resolve telemetry.db — Command: `bash -c "test -f packages/core/src/telemetry/mod.rs && grep -rq 'telemetry.db' packages/core/src/telemetry && echo ok"`
- [x] AC-5: leitura de economia não cruza mais com events (`agent.start` sumiu do reader) — Command: `node -e "const fs=require('fs');const s=fs.readFileSync('packages/core/src/economy/reader.rs','utf8');process.exit(/agent\.start/.test(s)?1:0)"`

## Tabela de Waves

| Wave | Spec | Role | Depende de | Resumo |
|------|------|------|------------|--------|
| 1 | [[wave-1-library]] | library | — | core: **módulo `telemetry/`** (SOLID, trait-backed) dono do `telemetry.db` independente — `usage_totals` reduzida + `run_usage` com atribuição load-bearing + `run_attribution` + migração (agrega, backfill via correlação única, dropa de mustard.db, VACUUM) |
| 2 | [[wave-2-general]] | general | [[1]] | rt: hook grava atribuição no `run_attribution`; coletor carimba o run na escrita via `telemetry::writer`; upsert de `usage_totals` reduzido — run nasce atribuído |
| 3 | [[wave-3-ui]] | ui | [[1]] | dashboard + economy reader consomem `telemetry::reader`; **remover a CTE de JOIN com events** — `GROUP BY` direto no run auto-atribuído; medições preservadas; sem ATTACH |

## Critique Coverage

| Item levantado | Categoria | Onde |
|---|---|---|
| "Toda telemetria separada" | Coberto | Waves 1-3 — `claude_code_otel` + `spans` + mapa de atribuição → `telemetry.db` independente |
| "Por que events precisa de telemetria?" | Coberto | Não precisa — Wave 2 carimba atribuição na escrita; Wave 3 remove o JOIN com events |
| "Ajustada p/ não gerar dado irrelevante" | Coberto | Wave 1 — schema reduzido (dropa minuto/token_type/attrs/count/signal) |
| "Só valores por projeto e valor total" | Coberto | DBs já por-projeto; custo total/por-modelo/sessão preservados na Wave 3 |
| "Telemetria alimenta outras medições" | Coberto | Inventário mapeou consumidores; Não-Objetivo de não alterar medições; Wave 3 preserva |
| "Analise essa parte" | Coberto | ANALYZE: inventário exaustivo + análise do acoplamento span↔events |
| Eventos efêmeros (tool.result) na `events` | Não-Objetivo | Frente separada — não é telemetria OTEL; deferida |
