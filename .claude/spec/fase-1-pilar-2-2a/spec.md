---
id: spec.fase-1-pilar-2-2a
---

# Fase 1 do Pilar 2 (2a): adicionar uma janela de tempo (1d/7d/15d/30d) a visao de Economia do dashboard, hoje sem recorte temporal. O EconomyScope (packages/core) ganha um filtro de janela que COMPOE com o escopo atual (Projeto/Spec/Wave/Comparar); os readers de economia (reader.rs) filtram os eventos NDJSON por ts dentro da janela; o EconomyScopeDto + os 6 comandos dashboard_economy_* (apps/dashboard/src-tauri/telemetry.rs) passam a janela adiante; e o Economia.tsx ganha um seletor 1d/7d/15d/30d ao lado do ScopeBar. Deterministico, fail-open. Codigo em ingles.

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Fase 1 do Pilar 2 (2a): adicionar uma janela de tempo (1d/7d/15d/30d) a visao de Economia do dashboard, hoje sem recorte temporal. O EconomyScope (packages/core) ganha um filtro de janela que COMPOE com o escopo atual (Projeto/Spec/Wave/Comparar); os readers de economia (reader.rs) filtram os eventos NDJSON por ts dentro da janela; o EconomyScopeDto + os 6 comandos dashboard_economy_* (apps/dashboard/src-tauri/telemetry.rs) passam a janela adiante; e o Economia.tsx ganha um seletor 1d/7d/15d/30d ao lado do ScopeBar. Deterministico, fail-open. Codigo em ingles..

Âncoras (do scan):
- apps/dashboard/src/features/economy/ScopeBar/index.tsx (economy, scope, dashboard)
- packages/core/src/domain/economy/reader.rs (economy, scope, reader)
- apps/dashboard/src-tauri/src/process_util.rs (window, dashboard)
- apps/rt/src/commands/economy/metrics.rs (economy, window)
- apps/cli/src/commands/install_nerd_font.rs (window)
- apps/mcp/src/lib.rs (economy, scope)
- apps/dashboard/src/hooks/useEconomySummary.ts (economy, scope, dashboard)
- apps/dashboard/src/lib/types/economy.ts (economy, scope, dashboard, economyscope)
- apps/dashboard/src/lib/time.ts (time, dashboard)
- apps/dashboard/src/api/promptEconomy.ts (economy, time, dashboard)
- packages/core/src/domain/economy/scope.rs (economy, scope, economyscope)
- apps/dashboard/src/hooks/usePromptEconomy.ts (economy, dashboard)

Fatias recorrentes (precedente a espelhar): Cmd+Summary+summary (×4), path+Result (×2)

**Por que agora.** A visão de Economia hoje soma tudo desde sempre, sem recorte temporal — é difícil ler tendência ("o custo caiu nos últimos 7 dias?"). O `EconomyScope` (packages/core) já foi PROJETADO para isso: seu comentário diz literalmente que uma variante `TimeWindow` pode ser adicionada sem quebrar os `match` (`#[non_exhaustive]`). O `ScopeBar` já dá o escopo por Projeto/Spec/Wave/Comparar; falta só a dimensão de tempo, que compõe com qualquer escopo. É a Fase 1 do Pilar 2 (a lente de qualidade é a Fase 2, spec separada).

## Usuários/Stakeholders

O desenvolvedor que usa o dashboard para acompanhar a economia (custo/tokens) do processo guiado por especificação — hoje só enxerga o total acumulado, sem conseguir isolar um período recente.

## Métrica de sucesso

Na página Economia, o usuário escolhe uma janela (1d/7d/15d/30d) e os números (custo, tokens, por-agente, por-spec) passam a refletir **apenas** os eventos daquele período, **compondo** com o escopo já existente (Projeto/Spec/Wave/Comparar). Sem janela selecionada, o comportamento é idêntico ao de hoje (todos os eventos).

## Não-Objetivos

- A lente de **qualidade** por janela (AC-de-primeira, rodadas, escalonamento) — é a **Fase 2** do Pilar 2, uma spec separada.
- Mudar a **atribuição de custo por-agente** (o join heurístico sobre OTEL) — permanece como está.
- **Novas** métricas de economia — este corte só adiciona o recorte temporal às métricas que já existem.
- Uma janela "tudo" ou personalizada além das quatro fixas (1d/7d/15d/30d).

## Critérios de Aceitação

- **AC-1** — when um reader de economia recebe uma janela `[from, to]`, then eventos NDJSON com `ts` fora da janela são excluídos do agregado (e os de dentro permanecem).
  Command: `cargo test -p mustard-core -- economy_time_window`
  Expect: `test result: ok`
- **AC-2** — when nenhuma janela é dada (ou um evento não tem `ts` parseável), then o reader agrega todos os eventos como hoje — fail-open, sem regressão de escopo.
  Command: `cargo test -p mustard-core -- economy_time_window_absent`
  Expect: `test result: ok`
- **AC-3** — when um comando `dashboard_economy_*` recebe uma janela no `EconomyScopeDto`, then ele a repassa ao reader do core e o resultado reflete só o período.
  Command: `cargo test --manifest-path apps/dashboard/src-tauri/Cargo.toml -- economy_window`
  Expect: `test result: ok`
- **AC-4** — when a página Economia é renderizada, then o seletor de janela expõe exatamente as quatro opções (1d/7d/15d/30d) e trocar a opção re-consulta a economia com o novo recorte (o `from` derivado compõe com o escopo).
  Command: `pnpm --dir apps/dashboard build`
- **AC-5** — o build e os testes do workspace passam verdes.
  Command: `cargo build --workspace`

<!-- PLAN -->

## Arquivos

Core (janela + filtro):
- `packages/core/src/domain/economy/scope.rs` — o mecanismo de janela (`TimeWindow` previsto no enum, ou um limite `[from,to]` que compõe com o escopo)
- `packages/core/src/domain/economy/reader.rs` — os ~9 readers filtram os eventos NDJSON por `ts` dentro da janela

Dashboard — backend Tauri (repassar a janela):
- `apps/dashboard/src-tauri/src/telemetry.rs` — `EconomyScopeDto` + os 6 comandos `dashboard_economy_*`

Dashboard — frontend (seletor + fiação):
- `apps/dashboard/src/lib/types/economy.ts` — a janela no tipo de escopo do front
- `apps/dashboard/src/features/economy/ScopeBar/index.tsx` — o seletor 1d/7d/15d/30d ao lado do escopo (novo)
- `apps/dashboard/src/pages/Economia.tsx` — o estado da janela + fiação nas queries
- `apps/dashboard/src/lib/dashboard.ts` — os wrappers `invoke()` da economia passam a janela
- `apps/dashboard/src/hooks/useEconomySummary.ts` — a query inclui a janela na `queryKey`

## Limites

IN: recorte temporal (1d/7d/15d/30d) das métricas de economia **existentes**, compondo com o escopo (Projeto/Spec/Wave/Comparar); atravessa core + src-tauri + frontend. A janela é calculada na borda (front, via `dayjs`) e passada como intervalo; o core filtra determinístico dado o intervalo.
OUT: a lente de qualidade (Fase 2); qualquer métrica nova; a atribuição de custo por-agente; uma janela "tudo"/personalizada; migração de dados.