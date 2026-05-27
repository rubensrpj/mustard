# Mustard v4 — Fundação: gate de regressão de comportamento + waves + vocabulário

### Stage: Analyze
### Outcome: Active
### Flags: paused-redesign
### Scope: full
### Lang: pt-BR
### Checkpoint: 2026-05-27T17:56:09.926Z

> **⏸ PAUSADA em 2026-05-27 após W1 verde.** Motivo: o módulo `ast::*` definido nas waves W1.5 (subset mínimo tree-sitter com 3 grammars hardcoded), W2 (snapshot via `extract_function_signatures`) e W4 (gate Camada 2) conflita com a primícia [`feedback_mustard_agnostic`] reforçada pelo user em 2026-05-27 ("mustard é agnóstico, isso é primícia"). A doc original (`05-design/context7-extraction.md:188-196` + `06-specs/spec-A-foundation.md:197-204`) autoriza o hardcode como "mínimo viável" mas isso é DÉBITO agnóstico. **Próximo passo: redesign phase** — revisar fronteira A/C, repensar Camada 2 do gate, atualizar `context7-extraction.md` + `gate-regression.md` + `spec-A-foundation.md`. Entregues e committed (preservar): W0 (`packages/core/src/spec/touched_functions.rs` + fixtures) + W1 (`packages/core/src/vocabulary/` + `.claude/vocab/regression.toml`). Reabrir com `/mustard:spec` quando o design estiver alinhado.

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
- **Grammars tree-sitter para Python/Go/C#/Java** — W1.5 entrega só Rust + TypeScript + JavaScript (mínimo viável); resto vai pra Spec C
- **Reescrita do `subagent_inject` ou `agent_prompt_render`** — apenas extensão herdada da no-sqlite

## Critérios de Aceitação

Critérios binários (pass/fail), executáveis e independentes. Comandos shell são POSIX (`bash -c '…'`) ou Node (`node -e "…"`) para portabilidade Windows/POSIX. Quando o command depende de fixture ou módulo ainda não construído pela wave alvo, o AC declara `Command: TBD-em-wave-<N>` (M9 — nunca apresentar como pronto se não estiver).

- [ ] AC-A-1: Caso W6 reproduzido em fixture (capturada em W0) dispara o gate de regressão em ≥3 dos 4 pontos críticos (Momento 1, Momento 2, Momento 3, span-level) — Command: TBD-em-wave-7 (fixture W0 + gate W4 + span-level W5)
- [ ] AC-A-2: Plano que contém os tokens `fail-open` ou `empurrar pra W…` dispara o Momento 1 (pré-edit) antes de qualquer chamada Edit — Command: TBD-em-wave-4 (vocabulário W1 + gate W4)
- [ ] AC-A-3: Diff com `fn X() -> Option<T> { None }` em função pública declarada como preservada em `## Funções tocadas` dispara o Momento 2 (durante o diff) — Command: TBD-em-wave-4 (AST W1.5 + gate W4)
- [ ] AC-A-4: Foto antes/depois captura função que esvaziou (antes 23 entradas no corpo, depois 0) e reporta na consolidação da wave — Command: TBD-em-wave-7 (snapshot W2 + integração W4)
- [ ] AC-A-5: Span-level eval roda a cada `SubagentStop` (filho retornar), nunca acumula até o fim da wave — Command: TBD-em-wave-5 (hook `subagent_inject` estendido)
- [ ] AC-A-6: Verdict amarelo do gate PERGUNTA ao usuário (via AskUserQuestion) — não passa em silêncio — Command: TBD-em-wave-4
- [ ] AC-A-7: Verdict vermelho do gate BLOQUEIA a consolidação da wave (impede emissão de `pipeline.status` Completed) — Command: TBD-em-wave-4
- [ ] AC-A-8: `_summary.md` gerado por wave tem as 7 seções obrigatórias do schema (objetivo, herança, decisões, código, AC, verdict, próximos passos) — Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-rt wave_summary::test_required_sections'` (entregue em W3)
- [ ] AC-A-9: `_context.md` da wave N+1 tem ≤8.000 palavras quando gerado contra spec com 12 waves anteriores — Command: TBD-em-wave-3 (`wave_context::build` com fixture)
- [ ] AC-A-10: `resume-bootstrap` em spec com 12 waves usa ≤10.000 tokens (medido pelo orçamento exportado) — Command: TBD-em-wave-6 (`resume_bootstrap::run` estendido + medição)
- [ ] AC-A-11: `mustard_core::vocabulary::scan` casa 10.000 caracteres contra 100 termos em <5ms (bench) — Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-core --release vocabulary::bench::scan_10k_chars_100_terms'` (entregue em W1)
- [ ] AC-A-12: `mustard_core::regression_check::compare_snapshots` compara 100 funções em <50ms (bench) — Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-core --release regression_check::bench::compare_100_functions'` (entregue em W2)
- [ ] AC-A-13: Vocabulário em 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) é editável via `.claude/vocab/regression.toml` sem recompilar o binário — Command: TBD-em-wave-1 (load runtime via `VocabularyMatcher::from_layers`)
- [ ] AC-A-14: Promoção de termo entre camadas SEMPRE pergunta ao usuário (AskUserQuestion); nunca silencioso — Command: TBD-em-wave-1
- [ ] AC-A-15: Spec sem `## Funções tocadas` → fallback usa funções públicas tocadas pelo diff; gate funciona sem panic em fixture de spec legada — Command: TBD-em-wave-0 (fixture + W4 integração)
- [ ] AC-A-16: `mustard_core::ast::detect_stub_patterns` detecta corpo `None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()` em função pública declarada como preservada — Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-core ast::stub_detect::test_detect_all_patterns'` (entregue em W1.5)
- [ ] AC-A-17: `mustard_core::ast::TreeSitterParser::for_language` aceita `rust`, `typescript`, `javascript` sem panic; linguagem desconhecida retorna `Err` sem panic — Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-core ast::parser::test_supported_languages_and_unknown_errors'` (entregue em W1.5)

## Plano

Decomposição em **10 waves** (W0, W1, W1.5, W2, W3, W4, W5, W6, W7, W8) com cap rígido de ≤5 arquivos por wave (compat com a régua estabelecida na no-sqlite). W0 captura a fixture do caso W6 e o formato canônico de `## Funções tocadas`. W1, W1.5, W2 entregam os 3 primitivos de `mustard-core` (vocabulário, AST mínimo, snapshot). W3 escreve o formato canônico de `_summary.md` e `_context.md` por wave. W4 conecta os 3 primitivos no gate run-based (3 momentos × 3 camadas). W5 estende `subagent_inject` para span-level eval por filho. W6 estende `resume_bootstrap` com pruning de orçamento. W7 roda a spec inteira contra a fixture do W0 e ajusta thresholds. W8 fecha com QA-functional cobrindo todos os AC binários e quality-ledger inaugural. Detalhe completo em [`wave-plan.md`](./wave-plan.md).

## Funções tocadas

> Formato canônico em [`funcoes-tocadas.md`](./funcoes-tocadas.md). Status NOVO/ESTENDIDO/MODIFICADO segue R1-R6 do formato definido pela Fase A e validado pelo `spec-validate` na própria Spec A (auto-validação, AC-FT-6).

### Em `packages/core/src/vocabulary/` (NOVO)
- `vocabulary::VocabularyMatcher::from_layers`
- `vocabulary::VocabularyMatcher::scan`
- `vocabulary::VocabLayer::parse_from_toml`

### Em `packages/core/src/ast/` (NOVO — W1.5 subset mínimo)
- `ast::TreeSitterParser::for_language`
- `ast::TreeSitterParser::parse`
- `ast::extract_function_signatures`
- `ast::detect_stub_patterns`

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

## Dependências externas

Crates Rust adicionados ao workspace `Cargo.toml`:

- `aho-corasick = "1.1"` — vocabulário (W1)
- `tree-sitter = "0.22"` — AST multi-linguagem (W1.5, subset mínimo)
- `tree-sitter-rust = "0.21"` — grammar Rust (W1.5)
- `tree-sitter-typescript = "0.21"` — grammar TS (W1.5)
- `tree-sitter-javascript = "0.21"` — grammar JS (W1.5)
- `similar = "2"` — diff para `regression_check` (W2)
- `serde_json` — serialização canônica de snapshots (já presente)

Grammars adicionais (Python, Go, C#, Java) ficam para Spec C (Fase C — `context7-extraction.md`).

## Limites

- **DELETE:** nada — a fundação Rust pós-no-sqlite está saudável; v4 só estende.
- **REWRITE:** nenhum arquivo existente do v3 (apenas ESTENDE módulos herdados).
- **MODIFY:** `subagent_inject`, `agent_prompt_render`, `resume_bootstrap` (heranças diretas da no-sqlite).
- **CREATE:**
  - `packages/core/src/vocabulary/` (2 arquivos — W1)
  - `packages/core/src/ast/` (4 arquivos — W1.5 subset mínimo)
  - `packages/core/src/regression_check/` (3 arquivos — W2)
  - `packages/core/src/spec/touched_functions.rs` (1 arquivo — W0)
  - `apps/rt/src/run/{gate_regression_check, wave_summary, wave_context}.rs` (3 arquivos — W3, W4)
  - `apps/rt/src/hooks/pre_edit_intent_check.rs` (1 arquivo opcional — W4, alternativa run-based)
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

<!-- wikilinks-footer-start -->
- [feedback_refactor_no_stub_deferral](?) ⚠ não resolvido
- [feedback_no_stub_fail_open.md](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->