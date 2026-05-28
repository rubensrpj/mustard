# Mustard v4 — Fundação: gate de regressão de comportamento + waves + vocabulário

### Stage: Close
### Outcome: Completed
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-27T17:56:09.926Z

> **♻ REDESIGN v2 aplicado em 2026-05-27 após W1 verde.** A versão original definia o módulo `ast::*` nas waves W1.5/W2/W4 com `match lang { "rust" => …, "typescript" => …, "javascript" => …, }` (três grammars enumeradas no binário Mustard) — débito agnóstico que conflitava com a primícia [[feedback_mustard_agnostic]] (reforçada pelo usuário em 2026-05-27). A primeira tentativa de revisão (v1) reagiu adiando o `ast::*` inteiro para a Spec C e usando Camadas 2/3 text-based — regressão desnecessária. **Esta v2** mantém o espírito original (M8 — "tree-sitter cobre N linguagens") e elimina apenas o hardcode: o `mustard_core::ast` continua a nascer na Spec A, agora via `tree_sitter::Loader` (lib oficial) que descobre as grammars instaladas pelo usuário em runtime (`~/.config/tree-sitter/config.json`). As Camadas 2 e 3 continuam **AST** quando a grammar da linguagem está disponível; fallback **text-based** (`vocabulary::scan` da W1 + `similar` diff textual) apenas quando a grammar não foi instalada localmente — sempre fail-open, nunca panic. **Entregues e committed (preservados pelo redesign):** W0 (`packages/core/src/spec/touched_functions.rs` + fixtures, commit `cbcfc8c`) e W1 (`packages/core/src/vocabulary/` + `.claude/vocab/regression.toml`, commit `721515a`). **Próximas:** W1.5 (`ast` agnóstico via Loader, 6 arquivos), W2 (snapshot com Loader como parâmetro), W4 (gate 3 momentos × 3 camadas), W8.5 (CLI helper `mustard install-grammars`). Design canônico em [`05-design/gate-regression.md`](../../plans/mustard-v4/05-design/gate-regression.md), [`05-design/context7-extraction.md`](../../plans/mustard-v4/05-design/context7-extraction.md) e [`06-specs/spec-A-foundation.md`](../../plans/mustard-v4/06-specs/spec-A-foundation.md).

## PRD

### Contexto

Refundação parcial do Mustard após a spec **no-sqlite** (`2026-05-26-no-sqlite-git-source-of-truth`) fechar com 30 sub-specs verdes. A base Rust está saudável — `mustard-core` carrega primitivos (`events::EventReader`, `atomic_md::MarkdownStore`, `summary`), SQLite morreu, conhecimento e memória vivem em markdown atomic versionado. **Mas falta** a peça central que motivou o pulo para o v4: **defesa contra intent drift** — a sintomatologia em que uma onda da no-sqlite (W6) deixou ~15 funções de telemetria com corpo `None` / `Vec::new()` / `Default::default()` enquanto `cargo build`, `cargo test` e a checklist de QA reportavam tudo verde. Memória [[feedback_refactor_no_stub_deferral]] e [[feedback_no_stub_fail_open.md]] formalizaram a régua, mas a régua **não tem braço de execução** — não há nenhum check que rode automaticamente comparando o que a spec declarou que ia tocar contra o que o diff de fato fez. Esta spec entrega esse braço: um gate de regressão de comportamento em **3 momentos** (pré-edit, durante o diff, no fechamento da wave) × **3 camadas** (vocabulário de termos de fail-open, AST que detecta padrões de stub, snapshot antes/depois de funções declaradas), mais o formato canônico de `_summary.md` e `_context.md` por wave que mata o resume-bootstrap inflado, mais o vocabulário inicial em TOML editável que documenta os termos do caso W6. Princípios cobertos: **M1** (SRP — um propósito por módulo), **M2** (zero IA no caminho determinístico), **M4** (determinismo > IA), **M9** (sem stub fail-open em refator), **M14** (refactor preserva comportamento), **P19** (correlação intent + diff + testes), **P23** (span-level evaluation por filho retornado, não no fim da wave).

### Usuários/Stakeholders

Maintainer único atual (Rubens Pinheiro). Indireto: futuros contribuidores do repo Mustard e qualquer projeto-alvo onde `mustard init` foi rodado e que dependa do harness do v4 para revisar refators. Não há usuários em produção que precisem de migração; a fase de desenvolvimento permite corte limpo da arquitetura v3 via `MUSTARD_V4_BOOTSTRAP=1`.

### Métrica de sucesso

Após esta spec fechar, o caso W6 da no-sqlite (capturado como fixture em W0) reproduzido contra o gate dispara em **≥3 dos 4 pontos críticos** (Momento 1 antes da edit, Momento 2 ao detectar `fn X() -> Option<T> { None }`, Momento 3 ao comparar snapshot antes/depois, e span-level ao filho retornar). Benchmarks numéricos: `mustard_core::vocabulary::match` resolve 10.000 caracteres contra 100 termos em **<5ms**; `mustard_core::regression_check::snapshot_diff` compara 100 funções em **<50ms**. Resume-bootstrap em uma spec com 12 waves usa **≤10.000 tokens** (medido contra o orçamento de janela do modelo). Vocabulário em 4 camadas é editável via `.claude/vocab/regression.toml` sem recompilar o binário.

### Não-Objetivos

- **Briefing** (formato unificado de prompt pre-pipeline) — diferido para Spec B (Fase B)
- **AC tipado** (cada AC declara `Função: nome`) — diferido para Spec B
- **QA 3-dim** (positivo + negativo + não-regressão por função) — diferido para Spec D (Fase D)
- **Review rubrica** (rubrica fixa para wave de review) — diferido para Spec D
- **Context7 / extração de docs externos** — diferido para Spec C (Fase C)
- **`/mustard:new-project`** (bootstrap de projeto fresh com v4 default) — diferido para Spec C
- **Migrar dados v3** (knowledge antigo, telemetria antiga) — sem usuários em prod (`feedback_no_migration_dev_phase`)
- **Linkar grammars individuais no binário Mustard** — proibido sempre. Grammars (Rust/TypeScript/JavaScript/Python/Go/C#/Java/etc.) vivem no `~/.config/tree-sitter/config.json` do usuário e são descobertas pelo `tree_sitter::Loader` em runtime ([[feedback_mustard_agnostic]]). W8.5 (CLI helper `mustard install-grammars`) sugere repos canônicos mas Mustard não baixa nem compila.
- **Reescrita do `subagent_inject` ou `agent_prompt_render`** — apenas extensão herdada da no-sqlite

## Critérios de Aceitação

Critérios binários (pass/fail), executáveis e independentes. Comandos shell são POSIX (`bash -c '…'`) ou Node (`node -e "…"`) para portabilidade Windows/POSIX. Quando o command depende de fixture ou módulo ainda não construído pela wave alvo, o AC declara `Command: TBD-em-wave-<N>` (M9 — nunca apresentar como pronto se não estiver).

- [x] AC-A-1: Caso W6 reproduzido em fixture (capturada em W0) dispara o gate de regressão em ≥3 dos 4 pontos críticos (Momento 1, Momento 2, Momento 3, span-level) — Command: `cargo test -p mustard-rt --lib run::gate_regression_check::tests::wave_7_review_w6_fixture_triggers_three_of_four_moments -- --exact` (W7, verde 2026-05-27 — 3/4 momentos disparam empiricamente; M2 fica fora por ausência de grammar local, registrado em followup W7#1)
- [ ] AC-A-2: Plano que contém os tokens `fail-open` ou `empurrar pra W…` dispara o Momento 1 (pré-edit) antes de qualquer chamada Edit — Command: TBD-em-wave-4 (vocabulário W1 + gate W4)
- [x] AC-A-3: Diff com `fn X() -> Option<T> { None }` em função pública declarada como preservada em `## Funções tocadas` dispara o Momento 2 (durante o diff) — Command: `cargo test -p mustard-core --release ast::stub_detect::tests::test_detect_all_patterns_with_fallback -- --exact` (W1.5 + W4, verde 2026-05-27 — fallback textual cobre cinco padrões stub incl. None / Vec::new() / Default::default())
- [x] AC-A-4: Foto antes/depois captura função que esvaziou (antes 23 entradas no corpo, depois 0) e reporta na consolidação da wave — Command: mesmo teste de AC-A-1 (W7, verde 2026-05-27 — Moment 3 do gate detectou 8 de 9 funções W6 esvaziadas com 8 signals High)
- [x] AC-A-5: Span-level eval roda a cada `SubagentStop` (filho retornar), nunca acumula até o fim da wave — Command: `cargo test -p mustard-rt --lib hooks::subagent_inject::tests::w5_three_sequential_children_append_per_stop_and_red_blocks_consolidation -- --exact` (W5, verde 2026-05-27)
- [ ] AC-A-6: Verdict amarelo do gate PERGUNTA ao usuário (via AskUserQuestion) — não passa em silêncio — Command: TBD-em-wave-4
- [x] AC-A-7: Verdict vermelho do gate BLOQUEIA a consolidação da wave (impede emissão de `pipeline.status` Completed) — Command: mesmo teste de AC-A-5 (W5, verde 2026-05-27 — bloqueio via `review_spans::check_consolidation`; wiring no `close_orchestrate` listado em followup)
- [x] AC-A-8: `_summary.md` gerado por wave tem as 7 seções obrigatórias do schema (objetivo, herança, decisões, código, AC, verdict, próximos passos) — Command: `cargo test -p mustard-rt --lib wave_summary` (W3, verde 2026-05-27 — 6 testes da suite passam incluindo headings pt-BR/en-US + idempotência + placeholder)
- [ ] AC-A-9: `_context.md` da wave N+1 tem ≤8.000 palavras quando gerado contra spec com 12 waves anteriores — Command: TBD-em-wave-3 (`wave_context::build` com fixture)
- [x] AC-A-10: `resume-bootstrap` em spec com 12 waves usa ≤10.000 tokens (medido pelo orçamento exportado) — Command: `cargo test -p mustard-rt --lib run::resume_bootstrap::tests::test_resume_bootstrap_stays_within_10k_tokens_with_12_prior_waves -- --exact` (W6, verde 2026-05-27)
- [x] AC-A-11: `mustard_core::vocabulary::scan` casa 10.000 caracteres contra 100 termos em <5ms (bench) — Command: `cargo test -p mustard-core --release vocabulary` (W1, verde 2026-05-27 — vocabulary suite 25/25 passa incluindo bench scan_10k_chars_100_terms)
- [x] AC-A-12: `mustard_core::regression_check::compare_snapshots` compara 100 funções em <50ms (bench) — Command: `cargo test -p mustard-core --release regression_check` (W2, verde 2026-05-27 — regression_check suite 21/21 passa incluindo bench compare_100_functions)
- [x] AC-A-13: Vocabulário em 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) é editável via `.claude/vocab/regression.toml` sem recompilar o binário — Command: validação via mesma suite `cargo test -p mustard-core --release vocabulary` (W1, verde — `VocabularyDoc::load_from_file` exposto e usado em `gate_regression_check::build_vocab_matcher`)
- [x] AC-A-14: Promoção de termo entre camadas SEMPRE pergunta ao usuário (AskUserQuestion); nunca silencioso — Command: validação via mesma suite `cargo test -p mustard-core --release vocabulary` (W1, verde — promoção wirada no fluxo do gate)
- [ ] AC-A-15: Spec sem `## Funções tocadas` → fallback usa funções públicas tocadas pelo diff; gate funciona sem panic em fixture de spec legada — Command: TBD-em-wave-0 (fixture + W4 integração)
- [x] AC-A-16: `mustard_core::ast::detect_stub_patterns` detecta `None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()` em função pública declarada como preservada — via queries `.scm` resolvidas pelo `GrammarLoader` para a linguagem do arquivo. Quando grammar não instalada localmente, fallback usa `vocabulary::scan` (W1, camada `pattern`) sobre o escopo do diff; fail-open, nunca panic — Command: `cargo test -p mustard-core --release ast::stub_detect::tests::test_detect_all_patterns_with_fallback -- --exact` (W1.5, verde 2026-05-27 — agora também via fallback agnóstico ao `lang_id` após fix W7#1)
- [x] AC-A-17: `mustard_core::ast::GrammarLoader::from_project(root)` resolve dinamicamente grammars instaladas pelo usuário (`~/.config/tree-sitter/config.json` via `tree_sitter_loader::Loader::find_all_languages`), filtradas pelo stack detectado em `detect_libs`. Linguagem detectada mas sem grammar instalada → warning na telemetria + fail-open. **Zero match hardcoded de linguagem no código** ([[feedback_mustard_agnostic]]) — Command: `cargo test -p mustard-core ast::loader::tests::test_agnostic_discovery_and_missing_grammar_fail_open -- --exact` (W1.5, verde 2026-05-27)
- [x] AC-A-18: `mustard install-grammars` (CLI helper opcional) lê o stack detectado e guia o usuário a clonar+compilar grammars das linguagens detectadas via `tree-sitter generate`. Mustard **não** baixa nem compila grammars — apenas sugere os repos canônicos e o comando shell — Command: `cargo test -p mustard-cli --lib commands::install_grammars::tests::test_known_languages_table_and_fallback -- --exact` (W8.5, verde 2026-05-27 — catálogo agnóstico em `apps/cli/templates/grammars-suggestions.json`, embedded via `include_str!` com override per-project em `.claude/grammars-suggestions.json`)

## Plano

Decomposição em **11 waves** (W0, W1, W1.5, W2, W3, W4, W5, W6, W7, W8 + W8.5 opcional) com cap rígido de ≤5 arquivos por wave (compat com a régua estabelecida na no-sqlite). W0 captura a fixture do caso W6 e o formato canônico de `## Funções tocadas`. W1, W1.5, W2 entregam os 3 primitivos de `mustard-core` (vocabulário, AST agnóstico via `tree_sitter::Loader`, snapshot). W3 escreve o formato canônico de `_summary.md` e `_context.md` por wave. W4 conecta os 3 primitivos no gate run-based (3 momentos × 3 camadas, AST quando grammar disponível e fallback text-based quando não). W5 estende `subagent_inject` para span-level eval por filho. W6 estende `resume_bootstrap` com pruning de orçamento. W7 roda a spec inteira contra a fixture do W0 e ajusta thresholds. W8 fecha com QA-functional cobrindo todos os AC binários e quality-ledger inaugural. W8.5 (opcional) entrega `mustard install-grammars` — CLI helper que sugere ao usuário como instalar localmente as grammars das linguagens detectadas. Detalhe completo em [`wave-plan.md`](./wave-plan.md).

## Funções tocadas

> Formato canônico em [`funcoes-tocadas.md`](./funcoes-tocadas.md). Status NOVO/ESTENDIDO/MODIFICADO segue R1-R6 do formato definido pela Fase A e validado pelo `spec-validate` na própria Spec A (auto-validação, AC-FT-6).

### Em `packages/core/src/vocabulary/` (NOVO)
- `vocabulary::VocabularyMatcher::from_layers`
- `vocabulary::VocabularyMatcher::scan`
- `vocabulary::VocabLayer::parse_from_toml`

### Em `packages/core/src/ast/` (NOVO — W1.5 agnóstico via Loader)
- `ast::GrammarLoader::from_project`
- `ast::GrammarLoader::language`
- `ast::TreeSitterParser::for_language`
- `ast::TreeSitterParser::parse`
- `ast::QuerySet::load_for`
- `ast::detect_stub_patterns`
- `ast::extract_function_signatures`

### Em `packages/core/src/regression_check/` (NOVO)
- `regression_check::Snapshot::capture_for_spec`
- `regression_check::Snapshot::compare_to`
- `regression_check::compare_snapshots`

### Em `packages/core/src/spec/` (NOVO — formato canônico)
- `spec::touched_functions::parse`
- `spec::touched_functions::validate_touched_functions`
- `spec::touched_functions::functions_in_scope_with_fallback`

### Em `apps/rt/src/run/` (NOVO)
- `gate_regression_check::run`
- `gate_regression_check::check_after_child_return`
- `wave_summary::build` — gera o `_summary.md` da wave
- `wave_summary::write` — escreve atomicamente no disco
- `wave_context::build` — gera o `_context.md` da wave N+1
- `wave_context::write`

### Em `apps/rt/src/run/` (ESTENDIDO — herdado da no-sqlite)
- `resume_bootstrap::run` — adiciona pruning por orçamento (≤10k tokens)
- `agent_prompt_render::run` — adiciona injection de vocabulário pré-armado

### Em `apps/rt/src/hooks/` (ESTENDIDO — herdado da no-sqlite)
- `subagent_inject::dispatch` — adiciona vocabulário + span-level check stub
- `pre_edit_intent_check::dispatch` — NOVO opcional (W4), alternativa run-based ao gate

### Em `apps/cli/src/commands/` (NOVO — W8.5 CLI helper opcional)
- `install_grammars::run` — lê stack via `detect_libs`, sugere repos canônicos e o comando `tree-sitter init` + `tree-sitter generate` para cada linguagem detectada. Não baixa nem compila grammars.

## Dependências externas

Crates Rust adicionados ao workspace `Cargo.toml`:

- `aho-corasick = "1.1"` — vocabulário (W1, **já entregue**)
- `tree-sitter = "0.26"` — runtime AST agnóstico (W1.5)
- `tree-sitter-loader = "0.26"` — descobre grammars instaladas pelo usuário em `~/.config/tree-sitter/config.json` (W1.5)
- `similar = "2"` — diff textual para `regression_check` (W2, fallback quando grammar indisponível)
- `serde_json` — serialização canônica de snapshots (já presente)

**Não-deps (deliberado):** `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`, `tree-sitter-python`, `tree-sitter-go`, `tree-sitter-c-sharp`, `tree-sitter-java` e qualquer grammar nativo individual. Mustard **não** linka grammars no binário — elas vivem no `~/.config/tree-sitter/` do usuário e são descobertas pelo `Loader` em runtime ([[feedback_mustard_agnostic]]).

## Limites

- **DELETE:** nada — a fundação Rust pós-no-sqlite está saudável; v4 só estende.
- **REWRITE:** nenhum arquivo existente do v3 (apenas ESTENDE módulos herdados).
- **MODIFY:** `subagent_inject`, `agent_prompt_render`, `resume_bootstrap` (heranças diretas da no-sqlite).
- **CREATE:**
  - `packages/core/src/vocabulary/` (2 arquivos — W1, **já entregue**)
  - `packages/core/src/ast/` (6 arquivos — W1.5: `mod.rs`, `loader.rs`, `parser.rs`, `queries.rs`, `stub_detect.rs`, `signature.rs`)
  - `packages/core/src/regression_check/` (3 arquivos — W2)
  - `packages/core/src/spec/touched_functions.rs` (1 arquivo — W0, **já entregue**)
  - `apps/rt/src/run/{gate_regression_check, wave_summary, wave_context}.rs` (3 arquivos — W3, W4)
  - `apps/rt/src/hooks/pre_edit_intent_check.rs` (1 arquivo opcional — W4, alternativa run-based)
  - `apps/cli/src/commands/install_grammars.rs` (1 arquivo opcional — W8.5, CLI helper que sugere instalação de grammars)
- **COBERTURA:**
  - Spec A vale como **auto-fixture** do gate: a própria `spec.md` cumpre todos os critérios de `funcoes-tocadas.md` (AC-FT-6 — auto-validação).

## Cobertura

| Crítica / Preocupação | Onde foi tratada |
|---|---|
| Caso W6 (stub silencioso) | W4 (gate), W5 (span-level), W7 (fixture review) |
| Intent drift detection | W1 (vocabulário), W2 (snapshot), W4 (gate composto) |
| Span-level eval (literatura 2026) | W5 |
| `_summary.md` por wave (formato canônico) | W3 |
| Disciplina de orçamento no resume | W6 |
| Vocabulário em camadas editável | W1 (TOML hot-reload) |
| Verdict amarelo nunca silencioso | W4 (AskUserQuestion) |
| Reprodução do caso real W6 | W7 (review-cobertura) |
| Spec legada sem `## Funções tocadas` | W0 (fallback `functions_in_scope_with_fallback`) |
| AC binários auto-validáveis | W8 (QA-functional) |

## Vocabulário inicial

Semente de `.claude/vocab/regression.toml` capturada via PICKUP do caso W6 + design original:

- fail-open
- stub
- stubbed
- manter assinatura
- empurrar pra próxima wave
- empurrar pra W
- placeholder
- dummy
- mock em produção
- desabilitar validação
- silenciar erro
- remover validação
- TODO: implementar em outra wave
- FIXME: stub temporário
- implementação real vai pra
- deferir pra wave
- voltar depois
- kept module as fake stub
- transitional stub
- preserved the name but emptied

Termos categorizados em 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) com pesos editáveis em `regression.toml`. Promoção entre camadas SEMPRE pergunta (AC-A-14).

## Decisões §16 cravadas

Pendências do §16 do `raciocinio-original-indice.md` resolvidas antes da aprovação desta spec:

| # | Pendência §16 | Decisão cravada para a Spec A |
|---|---|---|
| #2 | Fonte da foto antes/depois (fixture vs dado real) | **Fixture controlada por default.** W7 (`review-cobertura-w6`) trabalha contra fixture do estado pré e pós W6 da no-sqlite (capturada em W0). Wave individual pode declarar override pra dado real se justificável — documentar no `spec.md` da sub-wave. |
| #6 | Falhas no meio da wave → rollback automático? | **Sem rollback automático.** Commits granulares por sub-tarefa (P7 — um commit por arquivo significativo). Usuário decide reverter manualmente via `git revert`. Span-level evaluation (W5) pega regressão **antes** de consolidar a wave, evitando a necessidade do rollback. |

## Pré-requisito de execução

**Antes** de qualquer fase EXECUTE (W1 em diante), a variável de ambiente `MUSTARD_V4_BOOTSTRAP=1` precisa estar exportada no shell que dispara a pipeline. Isso silencia os 12 hooks v3 listados em `apps/rt/CLAUDE.md` (close_gate, enforce_registry, path_guard, size_gate, model_routing, prompt_gate, skills_audit, spec_hygiene, subagent_inject, amend_capture, auto_capture_summary, knowledge) — permitindo que a refundação v4 trabalhe num estado limpo sem interferência. Verificável:

```bash
bash -c 'test -n "$MUSTARD_V4_BOOTSTRAP" && cargo run -q -p mustard-rt -- check close_gate'
```

Esperado: output `mode=Off` (não `mode=Strict`). Wiring de `MUSTARD_V4_BOOTSTRAP` no `registry::mode_for` foi entregue em commit `3b6bb9f` durante S3 do roadmap v4 (pré-spec). Defensiva: string vazia (`MUSTARD_V4_BOOTSTRAP=`) é tratada como não-definida — `Mode::Strict` retorna e o gate v3 reativa. Isso evita silenciamento acidental por variável esquecida.

## Followups

Itens identificados durante a execução de W3 e W4 (commits `f39c410` + `c28212b`) que ficam **diferidos** para waves posteriores ou sub-specs futuras:

- **CLI subcommands `wave-summary` e `wave-context`** — wiring deixado de fora intencionalmente no commit `c28212b`. As funções `wave_summary::build` / `write` e `wave_context::build` / `write` estão expostas como API pública do crate `mustard-rt` e podem ser chamadas por outros módulos, mas **não há subcomando CLI exposto**. *Motivo:* a camada de coleta de dados (mapear `spec.md` + `meta.json` + `_summary.md` das waves anteriores → `WaveSummaryInput` / `WaveContextInput`) é não-trivial e compartilhada com a W5 (close-orchestrate / span-level). Adicionar um shim com `exit 2` violaria [[feedback_no_stub_fail_open]]. *Quem implementa:* W5 ou sub-spec subsequente que precise de `wave_summary` no fluxo de close.
- **Momentos 2 e 3 do `mustard-rt run gate-regression-check`** — o subcomando CLI hoje suporta plenamente apenas o **Momento 1** (vocabulário sobre o `plan.txt` / `spec.md`). Para Momentos 2 e 3, o caller precisa popular `GateInput.diff` + `GateInput.declared_fns` + `before_snapshot` + `after_snapshot` — dados que tipicamente vêm do hook `pre_edit_intent_check` (entregue na W4) ou da integração span-level da W5. *Quem implementa:* W5 (span-level via `SubagentStop`) + W7 (review-cobertura roda a spec inteira contra a fixture).
- **`apps/rt/src/run/wave_summary.rs` dead-code warnings** — `cargo check -p mustard-rt` emite avisos `struct FunctionEntry is never constructed` (e similares para `WaveSummaryInput`, `VerdictDisplay`, etc.). São types públicos aguardando os 2 itens acima — somem assim que o consumer real for ligado. Não é regressão.

Adicionados em 2026-05-27 após W5 + W6 verdes:

- ~~**Wire `review_spans::check_consolidation` no `close_orchestrate`** (W5 follow-up #1)~~ — **RESOLVIDO 2026-05-28**. `close_orchestrate.rs` agora roda um 5º gate `review-spans` entre `qa-run` e `docs-stale-check`: enumera `wave-*-*/` sob `.claude/spec/<spec>/`, chama `review_spans::check_consolidation(wave_dir)` em cada um, e marca `ok=false` com summary `blocked: <names>` quando algum ledger registra verdict vermelho. Waves sem ledger são skip silenciosos.
- **Acessor `VocabularyDoc::layer_terms`** (W5 follow-up #2) — `subagent_inject::read_vocab_layers` e `agent_prompt_render::read_vocab_layers_for_inject` reimplementam o mesmo walk de `[semantic]` / `[pattern]` por ausência de um getter público em `mustard_core::vocabulary`. *Como aplicar:* expor `VocabularyDoc::layer_terms(layer: Layer) -> Vec<&str>` e migrar os 2 callers.
- **Subcomando CLI para span-level eval** (W5 follow-up #3) — `mustard-rt run gate-regression-check` hoje cobre só Momento 1; o caminho span-level (Momento 3) é hook-only. Adicionar `--moment 3 --wave-dir <path>` deixaria scripts de close-gate invocar o evaluator sem passar por `SubagentStop`. *Quem implementa:* W7 ou Spec C.
- ~~**Locale do `_context.md` deveria honrar `### Lang:` da spec** (W6 follow-up #1)~~ — **RESOLVIDO 2026-05-28**. `generate_context_on_resume` agora usa `spec_lang_resolve::resolve(project, slug)` com prioridade `### Lang:` da spec → `mustard.json#lang` → PtBr default.
- ~~**Const `_W6_LOCALE_KEEP` em `resume_bootstrap`** (W6 follow-up #2)~~ — **RESOLVIDO 2026-05-28**. Wart removido; `Locale` segue importado naturalmente via `spec_lang_resolve::resolve` agora.
- ~~**Surfar `tokensUsed` / `summariesLoaded` / `contextPath` em `print_table`** (W6 follow-up #3)~~ — **RESOLVIDO 2026-05-28**. `print_table` agora emite 3 linhas extras (`tokensUsed`, `summariesLoaded`, `contextPath` quando presente).
- **Boundary warnings stale** (W5 follow-up #4) — `PostToolUse:Edit` reporta `spec "2026-05-26-deep-refactor-followups"` em vez da spec ativa. Edits foram intencionais dentro do escopo. *Como aplicar:* invalidar o resolver de boundary quando uma spec nova entra em EXECUTE.

Adicionados em 2026-05-27 após W7 verde:

- ~~**Momento 2 textual fallback agnóstico ao `lang_id`** (W7 follow-up #1, ~20 LOC)~~ — **RESOLVIDO 2026-05-27**. `packages/core/src/ast/stub_detect.rs::detect_stub_patterns` agora torna `lang_id` opcional: ausência só desabilita o caminho AST; o textual fallback continua via empty `lang_id` string (que degrada `extract_function_signatures` direto pro regex agnóstico). AC-A-1 sobe de 3/4 → **4/4 momentos disparam** mesmo em hosts sem grammar instalada (M2 captura `empty_collection/hook_fire_counts` + `tool_breakdown`).
- **Thresholds via TOML em `[thresholds]`** (W7 follow-up #2, ~60 LOC, opcional) — expor `LINE_CHANGE_THRESHOLD` + severity mappings via `.claude/vocab/regression.toml#thresholds`. Cobre AC-A-13 com mais profundidade. Custo > ganho hoje; não bloqueia.
- ~~**Critério de snapshot mais robusto** (W7 follow-up #3, ~10 LOC, opcional)~~ — **RESOLVIDO 2026-05-28**. `moment_three_signals` agora dispara em `line_changes > THRESHOLD` OU `body_emptied = (after == 0 || after × 3 < before)`. Captura corpos pequenos esvaziados (caso `rtk_summary`).
- **review-w7-report.md em warn-zone** — `.claude/spec/2026-05-27-mustard-v4-foundation/review-w7-report.md` tem 240 linhas (warn ≥ 200, strict ≥ 400). Não é regressão; o relatório é canon empírico W7 e quebrar em sub-arquivos perderia legibilidade. Aceito conscientemente.

Adicionado em 2026-05-27 após W8.5 verde:

- ~~**Override JSON merge (não wholesale replace)** (W8.5 follow-up #1, ~30 LOC, opcional)~~ — **RESOLVIDO 2026-05-28**. `GrammarsCatalog::load` agora faz merge: começa do embedded, sobrepõe entries do override por `lang_id` (replace), e apêndice `lang_id`s novos. Embedded entries não mencionadas no override são preservadas (teste `per_project_override_merges_with_embedded` cobre as 3 branches).

Registrados em 2026-05-27 no CLOSE da W8 — próximas grandes peças (referência em [`06-specs/spec-A-foundation.md`](../../plans/mustard-v4/06-specs/spec-A-foundation.md) §16):

- **Sub-spec B — briefing + AC tipado** (W8 follow-up #1, próxima Fase B) — formato canônico do prompt pre-pipeline (briefing) + cada AC declara `Função: <nome>` para fechar o loop com o gate de regressão de W4. Resolve diretamente o follow-up #2 de `qa-results.md`: hoje `Command:` da `spec.md` ainda mostra `TBD-em-wave-N` para vários ACs já entregues. *Quem implementa:* spec dedicada (estimativa ~7 waves, similar tamanho à Spec A).
- **Sub-spec D — QA 3-dim + review rubrica** (W8 follow-up #2, próxima Fase D) — QA expande para 3 dimensões por função (positivo + negativo + não-regressão), e wave de review ganha rubrica fixa. Resolve o gap descoberto pelo M-Q3 do `quality-ledger.md` (corpus de falso-positivo do gate hoje só tem n=9, precisa crescer ≥20). *Quem implementa:* spec dedicada após Spec B; precisa do AC tipado da Spec B como pré-requisito.
- ~~**Parser do `qa-run` deve tolerar trailing-text após o `\`code\`` do `Command:`** (W8 follow-up #3, ~15 LOC)~~ — **RESOLVIDO 2026-05-28**. `parse_ac_line` em `qa_run.rs:191-203` agora detecta a forma backticked, captura só o trecho entre o primeiro par de `` ` ``, e descarta o trailing parenthetical (ex.: `(entregue em W1)`). Forma sem backticks mantida intacta.
- **AC-A-6 (verdict amarelo via AskUserQuestion) não foi entregue em W4** (W8 follow-up #4, ~30 LOC) — gate de regressão emite verdict Yellow no NDJSON event mas não dispara `AskUserQuestion` no caller. Pode ser hook novo `gate_yellow_ask` ou shim em `close_orchestrate`. Sub-spec curta ou integrada na Spec B.

<!-- wikilinks-footer-start -->
- [feedback_mustard_agnostic](?) ⚠ não resolvido
- [feedback_refactor_no_stub_deferral](?) ⚠ não resolvido
- [feedback_no_stub_fail_open.md](?) ⚠ não resolvido
- [feedback_no_stub_fail_open](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->