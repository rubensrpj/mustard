---
id: spec.matar-prd-standalone-fazer-feature
---

# Matar o PRD standalone e fazer o /feature grelhar inline: glossary-coverage detecta termos de dominio fracos, um mini-grill focado grava os termos confirmados no CONTEXT.md por-subprojeto via CONTEXT-MAP, e a spec passa a ser o PRD; remover prd-build e o skill mustard:prd, e religar a rota PRD do dashboard como porta GUI para o fluxo feature

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

Matar o PRD standalone e fazer o /feature grelhar inline: glossary-coverage detecta termos de dominio fracos, um mini-grill focado grava os termos confirmados no CONTEXT.md por-subprojeto via CONTEXT-MAP, e a spec passa a ser o PRD; remover prd-build e o skill mustard:prd, e religar a rota PRD do dashboard como porta GUI para o fluxo feature.

Âncoras (do scan):
- apps/dashboard/src/features/prd/IntentHero/index.tsx (feature, prd, lapidate, dashboard)
- apps/dashboard/src-tauri/src/prd_lapidator.rs (prd, lapidate, dashboard)
- apps/rt/src/commands/glossary_coverage.rs (glossary, coverage)
- packages/core/src/domain/economy/reader.rs (context, spec)
- apps/scan/src/ingest.rs (coverage)
- apps/mcp/src/lib.rs (spec)
- apps/cli/src/commands/git_flow.rs (spec)
- apps/dashboard/src/features/telemetry/TelemetryTimeRangeContext/index.tsx (feature, context, dashboard)
- apps/dashboard/src/api/prd.ts (prd, lapidate, dashboard)
- apps/dashboard/src/features/changeRequests/ChangeRequestActivityBlock/index.tsx (feature, spec, dashboard)
- apps/dashboard/src/features/amend/AmendActivityBlock/index.tsx (feature, spec, dashboard)
- apps/dashboard/src/features/telemetry/HistoryStrip/index.tsx (feature, spec, dashboard)

Fatias recorrentes (precedente a espelhar): Report (×7)

**Por que agora.** O PRD hoje é uma camada redundante. A rota PRD do dashboard lapida o intent com IA (Sonnet via `claude -p "/mustard:prd"`), joga o resultado no clipboard e o `/feature` **re-lapida tudo de novo** — o rascunho é descartado. Pior: o `prd-build` (núcleo determinístico) e a saída da rota são **write-only** — nenhuma parte de feature/scan/bugfix lê o PRD (confirmado por grep: zero leitores). Enquanto isso, a spec que o `/feature` gera **já tem a camada PRD** (as `PRD_SECTIONS`: context, users, metric, non-goals, acceptance-criteria). Então "PRD" não é um terceiro artefato — é a metade-requisitos de uma spec, produzida duas vezes.

O documento durável e por-projeto que o pipeline **de fato lê** já existe: o `CONTEXT.md` (glossário de domínio, fatiado por relevância e injetado em todo subagente via `subagent_inject.rs`). Ele está vazio porque depende de uma sessão de grilling **manual** (`grill-with-docs`) que ninguém roda — e o `/feature` apenas *sugere* grelhar (`glossary-nudge`: "never grills inline").

A consolidação: **a spec passa a ser o PRD**; o `/feature` **grelha inline** (versão leve) — usa o `glossary-coverage` (já determinístico) pra detectar termos de domínio fracos, faz um mini-grill focado, grava os termos **confirmados** no `CONTEXT.md` do subprojeto certo (via `CONTEXT-MAP.md`, cujo resolvedor `resolve_context_files` já existe) — e o PRD standalone é removido. O crescimento do `CONTEXT.md` fica limitado por desenho (glossário-só + atualiza-não-acrescenta + slice por relevância + split por subprojeto); poda/lint ativa fica como follow-up.

## Usuários/Stakeholders

- **Quem usa `/feature` e `/bugfix`** (o dev do projeto-alvo): ganha specs mais afiadas porque o intent é traduzido contra o glossário do projeto, e o `CONTEXT.md` cresce sozinho com os termos que cada feature toca.
- **Quem usa a rota PRD do dashboard**: deixa de copiar rascunho descartável pro clipboard; a rota passa a disparar o mesmo fluxo `/feature` e produzir uma spec rastreada.
- **Mantenedor do Mustard**: menos superfície duplicada (um caminho de lapidação em vez de dois), menos código morto (prd-build write-only sai).

## Métrica de sucesso

- **Zero referências vivas** a `prd-build` / `mustard:prd` no código de produção (grep retorna nada fora de histórico/specs/changelog).
- **Grilling alimenta o doc**: numa run de `/feature` que toca um termo de domínio fraco, ≥1 bloco de termo é gravado no `CONTEXT.md` do subprojeto correto e referenciado na seção `context` da spec.
- **Rota do dashboard rastreável**: o lapidador do dashboard dispara o fluxo `/feature` (spec materializada em `.claude/spec/`), não um rascunho de clipboard.
- **Sem regressão**: `cargo test` e o build do dashboard permanecem verdes; a injeção de `CONTEXT.md` no subagente continua fail-open quando o arquivo não existe.

## Não-Objetivos

- **Não** construir poda/lint do `CONTEXT.md` (dedup, drop de termo genérico, staleness) agora — é follow-up, só quando o glossário realmente inchar.
- **Não** transformar o grill inline num interrogatório: é uma versão **leve e limitada** (mini-grill focado nos termos fracos), não a entrevista implacável do `grill-with-docs` interativo.
- **Não** alterar o formato do `CONTEXT.md` nem o dos ADRs, nem o resolvedor multi-contexto `resolve_context_files` (já existe e funciona).
- **Não** remover `PRD_SECTIONS` do core — continuam sendo as seções canônicas da camada PRD **da spec**.

## Critérios de Aceitação

- **AC-1** — Build verde após as mudanças.
  Command: `cargo build`
- **AC-2** — `prd-build` removido do rt (sem registro nem módulo).
  Command: `rg -q "prd[-_]build|PrdBuild" apps/rt/src/commands && exit 1 || exit 0`
- **AC-3** — Skill `mustard:prd` removido.
  Command: `test ! -e apps/cli/templates/commands/mustard/prd/SKILL.md`
- **AC-4** — O `feature/SKILL.md` descreve o grill inline.
  Command: `rg -q "grelh|grill" apps/cli/templates/commands/mustard/feature/SKILL.md`
- **AC-5** — O escritor de termo grava no `CONTEXT.md` resolvido por `CONTEXT-MAP` (teste do novo módulo passa).
  Command: `cargo test -p mustard-rt grill_capture`
- **AC-6** — A autoria de PRD saiu do dashboard (sem página `/prd`, sem `prd_lapidator`) e o detalhe da spec ganhou o atalho de leitura (`slicePrdSection`).
  Command: `test ! -e apps/dashboard/src/pages/Prd.tsx && test ! -e apps/dashboard/src-tauri/src/prd_lapidator.rs && rg -q "slicePrdSection" apps/dashboard/src`
- **AC-7** — Sem referências pendentes aos símbolos PRD removidos (rt + dashboard). Suíte completa verde verificada nos reviews (`review.result` approved).
  Command: `rg -q "PrdReport|prd_build|lapidate_prd|trigger_feature|prd_lapidator|useLapidator" apps/rt/src apps/dashboard/src apps/dashboard/src-tauri/src && exit 1 || exit 0`

<!-- PLAN -->

## Arquivos

**Onda 1 — `/feature` grelha inline (rt + cli):**
- `apps/rt/src/commands/glossary_coverage.rs` — além de medir cobertura, expõe os termos fracos/ausentes pro grilling agir.
- `apps/rt/src/commands/grill_capture.rs` *(novo)* — grava um bloco de termo confirmado no `CONTEXT.md` resolvido por `resolve_context_files` (map-aware); glossário-só, atualiza-não-duplica, fail-open.
- `apps/rt/src/commands/mod.rs` — registra `grill-capture`.
- `apps/rt/src/commands/economy/context_slice.rs` — reusa/expõe o resolvedor multi-contexto pro lado escritor (sem alterar a leitura).
- `apps/rt/src/hooks/task/subagent_inject.rs` — `read_context_md` passa a resolver via `CONTEXT-MAP` (hoje lê um `CONTEXT.md` único).
- `apps/cli/templates/commands/mustard/feature/SKILL.md` — passo de grill inline no ANALYZE (substitui o nudge-só).
- `apps/cli/templates/refs/feature/glossary-nudge.md` — de "writes nothing / never grills" para o grill leve que grava confirmados.

**Onda 2 — remover PRD standalone (rt + cli + mcp):**
- `apps/rt/src/commands/spec/prd_build.rs` *(delete)*.
- `apps/rt/src/commands/spec/mod.rs` + `apps/rt/src/commands/mod.rs` — desregistrar `prd-build`.
- `apps/cli/templates/commands/mustard/prd/SKILL.md` *(delete)*.
- `apps/mcp/src/lib.rs` — remover exposição de `prd-build` se houver (varrer callers).
- `packages/core/src/domain/spec/contract.rs` — `PRD_SECTIONS` **permanece** (é da spec); só confirmar que nada mais referencia o `PrdReport`.

**Onda 3 — PRD sai do dashboard como autoria; entra como atalho de leitura na spec (revisada pós change-request):**
- *Removidos:* `apps/dashboard/src/pages/Prd.tsx`, `apps/dashboard/src/features/prd/IntentHero/index.tsx`, `apps/dashboard/src/hooks/useLapidator.ts`, `apps/dashboard/src/api/prd.ts`, `apps/dashboard/src/lib/types/prd.ts`, `apps/dashboard/src-tauri/src/prd_lapidator.rs` — a página/funil de autoria sai inteira.
- *Editados:* `apps/dashboard/src/App.tsx` (remove rota `/prd` + import), `apps/dashboard/src-tauri/src/lib.rs` (remove o handler `trigger_feature`), `apps/dashboard/src/lib/i18n.ts` (limpa chaves da autoria).
- *Adicionados:* `slicePrdSection` em `apps/dashboard/src/lib/` (corte determinístico `<!-- PRD -->`..`<!-- PLAN -->`) + aba **"PRD"** em `apps/dashboard/src/features/specs/SpecDetailDashboard/index.tsx` (via `SpecDrillDown`), reusando o `spec.md` já buscado — read-only, sem novo comando Tauri.

## Limites

IN: remover `prd-build` + skill `mustard:prd`; `/feature` grelha inline (versão leve) com escritor de termo map-aware; injeção de `CONTEXT.md` ciente do `CONTEXT-MAP`; rota PRD do dashboard religada ao fluxo `/feature`; `CONTEXT.md` por-subprojeto no lado escritor.
OUT: poda/lint do `CONTEXT.md` (follow-up); mudar o formato do `CONTEXT.md`/ADR; alterar o resolvedor `resolve_context_files` (já existe); reescrever o `grill-with-docs` interativo; remover `PRD_SECTIONS` do core.