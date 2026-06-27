---
id: spec.dashboard-aba-atividade-agrupar-trabalho
---

# dashboard aba atividade agrupar trabalho por tipo consumindo pipeline kind + narrativa do pedido

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

A aba "Specs" do dashboard só mostra o ESTÁGIO do trabalho que virou spec — então `/task` e `/bugfix` rápido ficam invisíveis e o usuário **perde a narrativa do que foi pedido** (fica às cegas sobre o histórico). Esta spec **substitui a aba Specs por "Atividade"**: agrupa TODO trabalho pelo TIPO e, em cada item, mostra o **pedido original + a narrativa** (pedido → fases → mudanças → desfecho).

Consome o evento `pipeline.kind` (criado pela spec `porta-unica-roteamento-linguagem-natural`, item #3) — **esta spec DEPENDE daquela executar primeiro** (o evento precisa existir). Rótulos humanos (mapeados do `kind`): Nova funcionalidade (feature·full), Ajuste/melhoria (feature·light), Correção (bugfix), Follow-up pontual (tactical-fix), Investigação (task analyze/audit/review/docs), Mudança rápida (task implement/refactor).

Âncoras (lado dashboard): `apps/dashboard/src-tauri/src/telemetry.rs` (deriva category/SpecCard + agrupa — o coração), `apps/dashboard/src/pages/Specs.tsx` (a aba a virar Atividade), `apps/dashboard/src/pages/Sessions.tsx` (modelo: já agrupa por category), `CollapsibleGroup`, hooks `useAggregate.ts`/`useTelemetryTimeline.ts`, projeção `packages/core/src/view/projection/timeline.rs`.

## Usuários/Stakeholders

- **Quem lê o dashboard** — vê o trabalho separado por tipo + reconstrói a história do pedido (deixa de ficar às cegas).
- (Indireto) quem audita o processo confiando no dashboard.

## Métrica de sucesso

- A aba "Atividade" substitui "Specs" e agrupa TODO trabalho (inclusive task e bugfix-rápido) por rótulo humano.
- Cada item mostra o pedido original + a narrativa (fases / mudanças / desfecho).
- `cargo test` (backend Tauri) verde; build do dashboard ok.

## Não-Objetivos

- **Não** cria o evento `pipeline.kind` (isso é a spec porta-única, #3) — esta apenas o CONSOME.
- **Não** muda o roteador / comandos.
- **Não** remove a aba Sessions (que já agrupa por `category`).

## Critérios de Aceitação

- **AC-1** — Build do backend verde
  Command: `cargo build`
- **AC-2** — Testes verdes
  Command: `cargo test`
- **AC-3** — Backend lê o evento de tipo
  Command: `grep -rq "pipeline.kind" apps/dashboard/src-tauri`
- **AC-4** — Aba "Atividade" existe
  Command: `grep -rq "Atividade" apps/dashboard/src/pages`
- **AC-5** — Rótulos humanos mapeados do kind
  Command: `grep -rq "Nova funcionalidade" apps/dashboard/src`

<!-- PLAN -->

## Arquivos

**Wave 1 — backend (Tauri/Rust + core):** `apps/dashboard/src-tauri/src/telemetry.rs` (ler `pipeline.kind` → expor `kind` por unidade de trabalho + agrupar), `apps/dashboard/src-tauri/src/watcher.rs`, `packages/core/src/view/projection/timeline.rs` (projeção com `kind` + narrativa do pedido).

**Wave 2 — frontend (React):** `apps/dashboard/src/pages/Specs.tsx` → vira "Atividade" (agrupa por rótulo humano + narrativa por item), modelando em `apps/dashboard/src/pages/Sessions.tsx`; `apps/dashboard/src/components/page/CollapsibleGroup`; hooks `apps/dashboard/src/hooks/useAggregate.ts` / `useTelemetryTimeline.ts`.

## Limites

IN: aba "Atividade" (substitui Specs) agrupando todo trabalho por rótulo humano; cada item com pedido original + narrativa; backend lê `pipeline.kind` e expõe `kind` + narrativa por unidade.
OUT: criar o evento `pipeline.kind` (é a spec porta-única #3); roteador/comandos; remover a aba Sessions.
DEPENDÊNCIA: requer a spec `porta-unica-roteamento-linguagem-natural` (#2+#3) executada — o evento `pipeline.kind` precisa existir.

## Concerns

- (WARN, analyze-validation) `pipeline.kind` aparece nos AC como **nome de evento**, não arquivo — falso positivo do validador.
- (WARN) `useTelemetryTimeline.ts` não localizado pelo scan — confirmar nome/caminho real do hook no EXECUTE (o digest o sugeriu como âncora).