---
id: spec.juiz-haiku-concerns-acima-shortlist
---

# juiz Haiku de concerns acima do shortlist deterministico no feature ANALYZE

<!-- drafter:tone=didactic — Write this spec narrative in didactic tone — expand abbreviations on first use (AC = Acceptance Criteria, wave = onda) and prefer plain words over jargon. -->

<!-- PRD -->

## Contexto

juiz Haiku de concerns acima do shortlist deterministico no feature ANALYZE.

Âncoras (do scan):
- apps/rt/src/commands/feature.rs (digest, feature)
- apps/dashboard/src/features/trace/ExecutionTrace/index.tsx (feature, model)
- packages/core/src/domain/scan.rs (digest, model)
- apps/scan/src/digest.rs (digest)
- apps/dashboard/src-tauri/src/telemetry_agg.rs (dispatch)
- apps/mcp/src/lib.rs (dispatch, model)
- apps/cli/src/cli.rs (dispatch)
- apps/rt/src/commands/agent/digest_adherence_finalize.rs (digest)
- apps/rt/src/hooks/task/main_context_counter.rs (dispatch, model)
- apps/rt/src/commands/pipeline/resume_bootstrap/dispatch_failure.rs (dispatch)
- apps/rt/src/commands/event/event_projections.rs (dispatch, model)
- apps/rt/src/util/sha256.rs (digest)

Fatias recorrentes (precedente a espelhar): Report (×7), args (×2). **Precedente a espelhar:** o padrão `shortlist determinístico em Rust → juiz uma camada acima que ESCREVE um arquivo → Rust LÊ com fallback determinístico` já existe em `context_inject.rs`/`agent_prompt_render.rs` (o `.memory-approved`: o juiz da orquestração grava o conjunto aprovado; o render lê, senão cai no recall determinístico). E o render determinístico de prompt de agente vive em `agent_prompt_render.rs`/`dispatch_plan.rs` (stub `MUSTARD-PROMPT-REF` expandido pelo hook).

**Por que agora.** A segmentação de concerns da consulta (separar um pedido multi-assunto em N sub-digests rotulados) é hoje feita por **co-ocorrência determinística DENTRO do `scan`** (`apps/scan/src/digest.rs` + `connected_components` em `rank.rs`) e **falha em prompt real** — provado nesta sessão contra o prompt real do sialia (3 concerns: ordenar datatable / múltiplos contatos do parceiro / regras de senha): colapsou em **1 blob (19 de 20 conceitos) + 1 nó-ruído**, porque termos comuns (`table`, `order`, `contact`, `validation`) co-ocorrem em muitos arquivos e o grafo vira componente conexo gigante. `connected_components` (união por qualquer módulo compartilhado) é maximamente permissivo.

A separação de concerns é um julgamento **semântico**, não estrutural. **Validado com teste nesta sessão:** um agente Haiku (modelo `haiku`, ~7,5s, ~28k tokens, sem ler nenhum arquivo), recebendo os MESMOS conceitos + os arquivos que cada conceito aponta (o shortlist determinístico que o digest já produz em `report.terms[].files`), particionou nos **3 concerns corretos** e ainda flagou o ruído (DTOs que casaram `sort`/`order` por acaso; `validation` como mecanismo genérico). Mesmo input: co-ocorrência = 1 blob; juiz Haiku = 3 concerns limpos.

A direção é mover a segmentação para um **juiz Haiku UMA CAMADA ACIMA** (na orquestração do `/feature`), espelhando o `.memory-approved`: o `scan`/digest segue **100% determinístico** (invariante — nunca chama modelo) e só EXPÕE o shortlist (conceitos + anchors por conceito); um comando Rust monta o prompt do juiz deterministicamente; o orquestrador despacha um agente Haiku que devolve a partição; o `/feature` decompõe por concern.

## Usuários/Stakeholders

- **O orquestrador / IA que roda `/feature`:** num pedido multi-assunto, recebe N concerns semânticos limpos (rótulo + conceitos + anchors) em vez de 1 lista diluída, e decompõe cada concern no trilho certo sem caçar à mão o que o ranking afogou.
- **O operador (indireto):** o pedido "3 coisas num prompt" vira 3 unidades de trabalho corretas desde o ANALYZE.
- **Quem evolui o digest:** o juiz semântico desacopla "achar arquivos" (determinístico, `scan`) de "agrupar por assunto" (semântico, Haiku) — cada um melhora sozinho.

## Métrica de sucesso

Dado o output do digest de um prompt multi-concern (fixture: o caso real do sialia — datatable / contatos-parceiro / login-senha), o passo do juiz produz **N concerns semânticos corretos** (cada concern com seu rótulo, seus conceitos e seus anchors), onde a co-ocorrência determinística produzia 1 blob. A parte determinística (montagem do prompt do juiz a partir de conceitos+anchors) é **byte-estável e testável**; o digest do `scan` permanece **determinístico** (zero chamada de modelo). O juiz é barato (Haiku, ~uma chamada por pesquisa multi-concern).

## Não-Objetivos

- **Pôr IA dentro do `scan`/digest:** invariante — o digest é 100% determinístico, nunca chama modelo. O juiz mora na orquestração.
- **Testar a saída do LLM de forma determinística:** o julgamento é não-determinístico; os critérios de aceitação cobrem a PLUMBING determinística (render do prompt + parse da resposta), espelhando como `agent-prompt-render` é testado (testa o render, não o agente).
- **Remover a co-ocorrência determinística do `scan` agora:** ela fica dormente/rebaixada a hint; a remoção do `connected_components`-como-resposta é follow-up separado, fora desta spec.
- **Mudar o algoritmo de ranking BM25F** ou a recuperação por conceito: só mudamos QUEM agrupa os conceitos em concerns.

## Critérios de Aceitação

- **AC-1** — Build do pipeline verde
  Command: `cargo build`
- **AC-2** — Suite completa verde (sem regressão)
  Command: `cargo test`
- **AC-3** — `concern-judge-render` monta um prompt de juiz determinístico e byte-estável a partir de um digest fixture (o caso sialia), contendo os conceitos + os arquivos por conceito + a instrução de particionar em concerns
  Command: `cargo test concern_judge_render`
- **AC-4** — O parser da resposta do juiz aceita a partição (JSON de concerns: rótulo + conceitos + anchors) e rejeita forma inválida sem quebrar
  Command: `cargo test concern_judge_parse`
- **AC-5** — O digest do `scan` permanece determinístico: nenhuma chamada de modelo é adicionada em `apps/scan` (guardrail verificável)
  Command: `cargo test concern_judge` 

<!-- PLAN -->

## Arquivos

- `apps/rt/src/commands/agent/concern_judge.rs` (novo) — comando `run concern-judge-render`: recebe o intent (ou o digest), reusa a recuperação determinística do digest (conceitos + `report.terms[].files`) e RENDERIZA o prompt do juiz (conceitos + anchors por conceito + contrato de particionar em concerns rotulados), byte-estável. Espelha `agent_prompt_render.rs`. Inclui o parser da resposta (JSON de concerns) com tolerância a forma inválida. Camada: **rt (comando)**.
- `apps/rt/src/commands/mod.rs` — registrar o subcomando novo nos DOIS pontos (variante `RunCmd` + braço `dispatch()`), conforme o Guard do rt. Camada: **rt (wiring)**.
- `apps/cli/templates/commands/mustard/feature/SKILL.md` — passo do juiz no ANALYZE/DECOMPOSE: gate de sinal multi-concern → `concern-judge-render` → dispatch de agente Haiku (modelo `haiku`) com o prompt renderizado → parse dos concerns → decompor por concern. Camada: **templates (orquestração)**.
- `apps/cli/templates/refs/feature/` (se precisar) — ref de progressive-disclosure do passo do juiz, se a prosa do SKILL ficar densa. Camada: **templates (ref)**.

## Limites

IN: comando `concern-judge-render` em apps/rt (render determinístico do prompt do juiz a partir de conceitos+anchors do digest + parser da resposta); passo do juiz na orquestração do `/feature` (gate multi-concern → render → dispatch Haiku → parse → decompor por concern); espelhar o padrão `.memory-approved`/`agent-prompt-render`; manter o digest do `scan` 100% determinístico.
OUT: IA dentro do `scan`/digest; remoção do `connected_components`-como-resposta (fica dormente; follow-up); mudança no BM25F/recuperação por conceito; teste determinístico da saída do LLM (testamos só a plumbing).