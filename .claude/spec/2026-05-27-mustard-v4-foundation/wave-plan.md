# Plano de ondas — Mustard v4 Fundação (gate de regressão + waves + vocabulário)

## Contexto

Refundação parcial pós-no-sqlite. A base Rust ficou saudável (primitivos `events`, `atomic_md`, `summary` em `mustard-core`; SQLite morto). Esta spec entrega o que faltou: **defesa contra intent drift** via gate de regressão de comportamento em 3 momentos × 3 camadas, formato canônico de `_summary.md` e `_context.md` por wave, e vocabulário inicial em TOML editável. Caso de uso de referência: W6 da no-sqlite, que deixou ~15 funções de telemetria com corpo zerado enquanto build/test/QA permaneciam verdes. Esta spec garante que esse caso falhe no gate.

## Decisões §16 cravadas (reiteração para clareza)

| # | Decisão |
|---|---|
| #2 | **Fixture controlada por default.** W7 trabalha contra fixture pré/pós W6 capturada em W0. Wave individual pode declarar override pra dado real, documentando no `spec.md` da sub-wave. |
| #6 | **Sem rollback automático.** Commits granulares por sub-tarefa (P7). Usuário decide reverter via `git revert`. Span-level eval (W5) pega regressão antes de consolidar — evita necessidade de rollback. |

## Hard rules (encoded em ACs e enforcement)

- **Zero stub fail-open** (M9 + M14 + [[feedback_no_stub_fail_open]] + [[feedback_refactor_no_stub_deferral]]). Wave que reduz comportamento via `None` / `Default::default()` / `Vec::new()` em função pública declarada como preservada falha no gate (Camada 2 — AST + W1.5).
- **Commit por sub-tarefa significativa** (P7). O orquestrador NÃO emite `pipeline.status` Completed pra wave inteira de uma vez; cada arquivo significativo vira commit.
- **Span-level antes de consolidar** (P23 + W5). Gate roda a cada `SubagentStop`; consolidação da wave é bloqueada se algum filho retornou verdict vermelho.
- **Cap rígido ≤5 arquivos por wave** (compat com a régua da no-sqlite — orçamento Opus ~30 tool uses, cada `.rs` pesado custa 2-5 tool uses). Wave com mais que isso quebra em sub-specs.
- **Idioma da spec uniforme em pt-BR** ([[feedback_spec_uniform_language]]); código e comentários em EN ([[project_code_language_policy]]).

## Pré-requisito de execução

`MUSTARD_V4_BOOTSTRAP=1` exportada antes de qualquer fase EXECUTE. Silencia os 12 hooks v3 via `registry::mode_for` (commit `3b6bb9f`). String vazia trata como não-definida (defensiva).

## Decomposição

| W | Nome | Role | Depende | Status |
|---|---|---|---|---|
| 0 | analyze-and-fixtures | mixed | — | 📋 |
| 1 | mustard-core-vocabulary | core | W0 | 📋 |
| 1.5 | mustard-core-ast-minimal | core | W0 | 📋 |
| 2 | mustard-core-regression-check | core | W0, W1.5 | 📋 |
| 3 | wave-summary-context-format | rt | W0 | 📋 |
| 4 | gate-regression-check-run | rt | W1, W1.5, W2 | 📋 |
| 5 | span-level-integration | rt | W4 | 📋 |
| 6 | resume-bootstrap-disciplined | rt | W3 | 📋 |
| 7 | review-cobertura-w6 | mixed | W4, W5 | 📋 |
| 8 | qa-and-close-followups | mixed | W1-7 | 📋 |

### W0 — analyze-and-fixtures (mixed)

Captura snapshot do PICKUP da no-sqlite como fixture (reprodução do caso W6 em estado pré e pós); identifica `apps/dashboard/src-tauri/src/telemetry.rs` no estado pré-W6 (23 entradas no corpo de funções) e pós-W6 (0 entradas — stubs); análise de dependências entre `vocabulary`, `ast`, `regression_check` e `gate_regression_check`. **Define formato canônico de `## Funções tocadas`** (parser + status NOVO/ESTENDIDO/MODIFICADO) — referência [`funcoes-tocadas.md`](./funcoes-tocadas.md). Insumo bloqueador para W2/W4/W5. Cria `packages/core/src/spec/touched_functions.rs` (parser + `validate_touched_functions` + `functions_in_scope_with_fallback`).

### W1 — mustard-core-vocabulary (core)

Arquivos: `packages/core/src/vocabulary/mod.rs` (NOVO), `packages/core/src/vocabulary/aho.rs` (NOVO). Wrapper SOLID sobre `aho-corasick`. Estruturas: `VocabularyMatcher::from_layers`, `VocabularyMatcher::scan`, `VocabLayer::parse_from_toml`. Suporte às 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) com pesos editáveis. Carrega `.claude/vocab/regression.toml` em runtime (hot-reload via mtime). Benchmark `vocabulary::bench::scan_10k_chars_100_terms` (<5ms — AC-A-11). ACs binários: AC-A-11, AC-A-13.

### W1.5 — mustard-core-ast-minimal (core)

Arquivos: `packages/core/src/ast/mod.rs`, `packages/core/src/ast/parser.rs`, `packages/core/src/ast/signature.rs`, `packages/core/src/ast/stub_detect.rs` (todos NOVO). Subset mínimo do `mustard_core::ast` para a Spec A. Estruturas: `TreeSitterParser::for_language` aceitando apenas `rust`, `typescript`, `javascript` (linguagem desconhecida retorna `Err` sem panic); `parse(source: &str) -> Result<Tree>`; `extract_function_signatures(tree: &Tree) -> Vec<FunctionSig>`; `detect_stub_patterns(tree: &Tree, functions: &[FunctionName]) -> Vec<StubMatch>` (detecta corpo `None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()`). Benchmark `ast::bench` (parse + extract <50ms para arquivo médio). Crates adicionados: `tree-sitter = "0.22"`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`. **Justificativa de existência da wave:** sem este subset, Camada 2 do gate (detecção de stub no diff) fica stubbed — quebra M9 e M14. Spec C completa o módulo com grammars Python/Go/C#/Java. ACs binários: AC-A-2, AC-A-3 (via Camada 2 do gate W4), AC-A-16, AC-A-17.

### W2 — mustard-core-regression-check (core)

Arquivos: `packages/core/src/regression_check/mod.rs`, `packages/core/src/regression_check/snapshot.rs`, `packages/core/src/regression_check/compare.rs` (todos NOVO). Foto antes/depois primitive. Estruturas: `Snapshot::capture_for_spec(spec_md, codebase) -> Snapshot`, `Snapshot::compare_to(other: &Snapshot) -> Diff`, `compare_snapshots(a, b) -> Vec<FunctionDelta>`. Serialização canônica JSON via `serde_json` (campos ordenados, bytes estáveis). Usa `similar = "2"` pra diff de corpo de função. Benchmark `regression_check::bench::compare_100_functions` (<50ms — AC-A-12). ACs binários: AC-A-4, AC-A-12.

### W3 — wave-summary-context-format (rt)

Arquivos: `apps/rt/src/run/wave_summary.rs`, `apps/rt/src/run/wave_context.rs`, `apps/cli/templates/skills/wave-summary-format.md` (todos NOVO). Schema do `_summary.md` com 7 seções obrigatórias (objetivo, herança, decisões, código, AC, verdict, próximos passos); schema do `_context.md` da wave N+1 (objetivo + herança + memória + posição no mapa + sugestão de próximos passos). Templates idempotentes via wikilinks. Geração atômica via `mustard_core::atomic_md::write_atomic`. ACs binários: AC-A-8, AC-A-9.

### W4 — gate-regression-check-run (rt)

Arquivos: `apps/rt/src/run/gate_regression_check.rs` (NOVO), `apps/rt/src/hooks/pre_edit_intent_check.rs` (NOVO opcional — alternativa run-based). Implementa os 3 momentos × 3 camadas: Momento 1 (pré-edit) lê o plano do agente + casa contra vocabulário W1; Momento 2 (durante o diff) roda `ast::detect_stub_patterns` em funções declaradas como preservadas; Momento 3 (fechamento) compara `Snapshot::capture_for_spec` antes e depois. Veredict verde/amarelo/vermelho: verde passa, amarelo dispara AskUserQuestion (AC-A-6), vermelho bloqueia consolidação (AC-A-7). ACs binários: AC-A-1, AC-A-2, AC-A-3, AC-A-6, AC-A-7.

### W5 — span-level-integration (rt)

Arquivos: `apps/rt/src/hooks/subagent_inject.rs` (ESTENDIDO — herda da no-sqlite), `apps/rt/src/run/agent_prompt_render.rs` (ESTENDIDO — herda). Gate roda a cada `SubagentStop` via `gate_regression_check::check_after_child_return`; verdict por filho registrado em `_review-spans.md` (atômico, append-only); bloqueio de consolidação se algum filho retornou vermelho. ACs binários: AC-A-5.

### W6 — resume-bootstrap-disciplined (rt)

Arquivos: `apps/rt/src/run/resume_bootstrap.rs` (ESTENDIDO — herda da no-sqlite). Orçamento ≤10.000 tokens por bootstrap; pruning via wikilinks (carrega só os `_summary.md` cujos wikilinks aparecem no contexto da wave atual); `_context.md` da wave atual gera-se on-resume via `wave_context::build` (W3). ACs binários: AC-A-10.

### W7 — review-cobertura-w6 (mixed)

Roda a spec inteira contra a fixture do W6 (capturada em W0); mede quantos pontos do gate disparam (espera-se ≥3 dos 4 — AC-A-1); ajusta thresholds e vocabulário inicial baseado nos disparos reais. Sem arquivos novos — só configuração + relatório. ACs binários: AC-A-1, validação consolidada.

### W8 — qa-and-close-followups (mixed)

QA-functional roda todos os AC binários (AC-A-1 a AC-A-17) via `mustard-rt run qa-run`; quality-ledger ganha entrada inaugural com snapshot de métricas de baseline; emissão de `pipeline.status` Completed; CLOSE da spec A. Sem arquivos novos — só validação e fechamento.

## Surpresa de naming W1.5

**Caveat de parser:** O parser `parse_wave_dir_number` em `apps/rt/src/run/dependency_precheck.rs:855` extrai dígitos ASCII contíguos após o prefixo `wave-` e para na primeira não-dígito. Logo:

- `wave-1_5-core/` → resolve para wave=1 (`_` quebra a sequência)
- `wave-1.5-core/` → resolve para wave=1 (`.` quebra a sequência)

Ambas as variantes **colidem** com `wave-1-core/` quando o parser scanneia o diretório da spec. Decisão pragmática desta spec: usar **`wave-1_5-core/`** (underscore é seguro em todos os filesystems — Windows + POSIX — enquanto ponto pode causar fricção em ferramentas que confundem com extensão). Convivência com W1 fica explícita: ambos os diretórios são auto-descritivos, e tooling downstream que precisar distinguir W1 de W1.5 deve consumir o `### Stage:` ou o nome literal do diretório, não o número parseado. Caveat documentado nesta wave-plan e na própria sub-spec `wave-1_5-core/spec.md`.

Fix de parser fica como follow-up: aceitar `\d+(_\d+)?` para suportar sub-waves explicitamente. Não bloqueia a Spec A.

<!-- wikilinks-footer-start -->
- [feedback_no_stub_fail_open](?) ⚠ não resolvido
- [feedback_refactor_no_stub_deferral](?) ⚠ não resolvido
- [feedback_spec_uniform_language](?) ⚠ não resolvido
- [project_code_language_policy](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->