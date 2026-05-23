# Tactical Fix: banner de ingestão estimada defasada

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Checkpoint: 2026-05-23T01:00:00Z
### Lang: pt
### Parent: [[2026-05-22-economia-didatica-e-economias-reais]]

## Contexto

Derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

Hoje a tabela `run_usage` (ESTIMADO, self-attributed) só recebe linhas via dois caminhos: o collector OTEL daemon (`mustard-rt` em modo collector) e uma migração one-shot do `mustard.db` legado. O caminho MEDIDO (`usage_totals`) é populado por um exporter separado da Anthropic.

Quando o collector está parado ou o OTEL não está configurado em Claude Code, `run_usage` congela mas `usage_totals` continua acumulando. A tela "Custo estimado por spec / onda" começa a mostrar dados antigos (ou só a spec mais recente que rodou via OTEL), e o usuário não tem sinal visual disso — só percebe quando estranha o número.

Fix: comparar `MAX(started_at)` de `run_usage` com `MAX(updated_at)` de `usage_totals`. Quando o estimado está mais que **6h** atrás do medido, exibir banner âmbar na página de Economia explicando que a tabela estimada está defasada e como ligar a ingestão.

## Decisão de design

- **Campo novo em `EconomySummary`**: `last_estimated_ms: Option<i64>` (epoch-ms) — análogo ao `last_updated_ms` existente. Reader popula via `MAX(started_at) FROM run_usage`.
- **Threshold de 6h**: balanceia ruído (sessões idle não disparam) com utilidade (parou de ingerir hoje cedo → banner já no fim da manhã).
- **UI**: banner âmbar acima do grid de KPI cards (não dentro da seção do estimado — o problema afeta múltiplos cards: per-spec, per-wave, distribuição por agente). Texto curto, link/instrução simples.
- **Graceful**: quando `last_estimated_ms` é `None` (ex.: tabela vazia) o banner não aparece — não-ingestão inicial não é o mesmo problema que ingestão-pausada.
- **Só escopo Projeto/AllProjects**: a comparação só faz sentido quando `last_updated_ms` está disponível (escopos não-filtrados).

## Arquivos

- `packages/core/src/economy/model.rs` — adicionar `last_estimated_ms: Option<i64>` em `EconomySummary` (aditivo, `#[serde(default)]`)
- `packages/core/src/telemetry/reader.rs` — adicionar `last_run_usage_ts(conn) -> Option<i64>` lendo `MAX(started_at) FROM run_usage`; fail-open
- `packages/core/src/economy/reader.rs` — popular `last_estimated_ms` no ramo unfiltered de `economy_summary`
- `apps/dashboard/src/lib/types/economy.ts` — espelhar `last_estimated_ms`
- `apps/dashboard/src/pages/Economia.tsx` — calcular delta e renderizar `<IngestionStaleBanner>` quando aplicável; threshold 6h

## Tarefas

### Library Agent (core)

- [x] `economy/model.rs`: `EconomySummary` += `last_estimated_ms: Option<i64>`, `#[serde(default)]`
- [x] `telemetry/reader.rs`: nova fn `last_run_usage_ts(conn: &Connection) -> Option<i64>` — `SELECT MAX(started_at) FROM run_usage`, fail-open ao nível de SQL
- [x] `economy/reader.rs`: no ramo `unfiltered` de `economy_summary`, popular `last_estimated_ms = telemetry::reader::last_run_usage_ts(tele.conn())`. No ramo `AllProjects`, agregar via `max()` sobre os per-project.
- [x] Manter `last_estimated_ms = None` quando escopo é Spec/Wave (mesma regra de `last_updated_ms`)
- [x] `cargo build && cargo test -p mustard-core --lib`

### UI Agent (dashboard)

- [x] `lib/types/economy.ts`: `EconomySummary` += `last_estimated_ms: number | null`
- [x] `pages/Economia.tsx`:
  - Calcular `staleHours = (last_updated_ms - last_estimated_ms) / 3_600_000` quando ambos não-null
  - Quando `staleHours > 6` e scope é project ou all_projects: renderizar `<IngestionStaleBanner hours={staleHours} />` antes do grid de KPI cards
  - Banner: ícone `AlertTriangle` (lucide), bg âmbar `bg-amber-500/10` + border `border-amber-500/30`, texto curto em PT explicando: "A tabela de custo estimado por spec/onda parou de receber dados há X horas. O custo medido continua atualizado. Para retomar: garanta que o collector do mustard-rt está rodando e que o Claude Code está exportando OTEL."
  - Botão/link discreto "saiba mais" abrindo `<details>` com 2-3 linhas extras de troubleshooting (opcional, manter compacto)
- [x] `pnpm --filter mustard-dashboard build`

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: campo novo presente no modelo — Command: `bash -c "grep -q 'last_estimated_ms' packages/core/src/economy/model.rs && echo ok"`
- [x] AC-4: reader expõe `last_run_usage_ts` — Command: `bash -c "grep -q 'last_run_usage_ts' packages/core/src/telemetry/reader.rs && echo ok"`
- [x] AC-5: build dashboard verde — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-6: banner referencia o problema — Command: `bash -c "grep -q 'parou de receber dados' apps/dashboard/src/pages/Economia.tsx && echo ok"`

## Limites

- Não tocar no schema do SQLite (campo já existe — `run_usage.started_at`)
- Não adicionar novo Tauri command (campo entra no `EconomySummary` existente, aditivo)
- Não mudar threshold dinamicamente — 6h fixo nesta entrega (ajuste fácil depois se ruidoso)
- Não tentar diagnosticar/auto-corrigir o collector — só sinalizar visivelmente
