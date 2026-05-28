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
| 1.5 | mustard-core-ast-loader-dynamic | core | W0 | 📋 — agnóstica via `tree_sitter::Loader`; zero grammars hardcoded |
| 2 | mustard-core-regression-check | core | W0, W1.5 | 📋 — AST quando grammar disponível, fallback `similar` text diff |
| 3 | wave-summary-context-format | rt | W0 | 📋 |
| 4 | gate-regression-check-run | rt | W1, W1.5, W2 | 📋 |
| 5 | span-level-integration | rt | W4 | 📋 |
| 6 | resume-bootstrap-disciplined | rt | W3 | 📋 |
| 7 | review-cobertura-w6 | mixed | W4, W5 | 📋 |
| 8 | qa-and-close-followups | mixed | W1-7 | 📋 |
| 8.5 | mustard-install-grammars (cli helper opcional) | cli | W1.5 | 📋 — opcional; orienta usuário a instalar grammars locais |

### W0 — analyze-and-fixtures (mixed)

Captura snapshot do PICKUP da no-sqlite como fixture (reprodução do caso W6 em estado pré e pós); identifica `apps/dashboard/src-tauri/src/telemetry.rs` no estado pré-W6 (23 entradas no corpo de funções) e pós-W6 (0 entradas — stubs); análise de dependências entre `vocabulary`, `ast`, `regression_check` e `gate_regression_check`. **Define formato canônico de `## Funções tocadas`** (parser + status NOVO/ESTENDIDO/MODIFICADO) — referência [`funcoes-tocadas.md`](./funcoes-tocadas.md). Insumo bloqueador para W2/W4/W5. Cria `packages/core/src/spec/touched_functions.rs` (parser + `validate_touched_functions` + `functions_in_scope_with_fallback`).

### W1 — mustard-core-vocabulary (core)

Arquivos: `packages/core/src/vocabulary/mod.rs` (NOVO), `packages/core/src/vocabulary/aho.rs` (NOVO). Wrapper SOLID sobre `aho-corasick`. Estruturas: `VocabularyMatcher::from_layers`, `VocabularyMatcher::scan`, `VocabLayer::parse_from_toml`. Suporte às 4 camadas (`semantic`, `pattern`, `keyword`, `noise`) com pesos editáveis. Carrega `.claude/vocab/regression.toml` em runtime (hot-reload via mtime). Benchmark `vocabulary::bench::scan_10k_chars_100_terms` (<5ms — AC-A-11). ACs binários: AC-A-11, AC-A-13.

### W1.5 — mustard-core-ast-loader-dynamic (core)

Arquivos: `packages/core/src/ast/mod.rs`, `packages/core/src/ast/loader.rs`, `packages/core/src/ast/parser.rs`, `packages/core/src/ast/queries.rs`, `packages/core/src/ast/stub_detect.rs`, `packages/core/src/ast/signature.rs` (todos NOVO). Módulo `mustard_core::ast` agnóstico desde o nascimento — **zero match hardcoded de linguagem**. Estruturas: `GrammarLoader::from_project(root)` usa `tree_sitter_loader::Loader::find_all_languages` para descobrir grammars instaladas pelo usuário em `~/.config/tree-sitter/config.json`, filtradas pelo stack detectado em `detect_libs` (lê manifests do projeto-alvo); `GrammarLoader::language(lang_id) -> Option<Language>` (`None` quando grammar não instalada — fail-open, não panic); `TreeSitterParser::for_language(loader, lang_id)` delega ao Loader sem `match` interno; `TreeSitterParser::parse(source) -> Result<Tree>`; `QuerySet::load_for(lang_id)` carrega queries `.scm` de `.claude/grammars/{lang_id}/queries/*.scm` (alimentadas por context7 na Spec C ou pelo helper W8.5); `detect_stub_patterns(loader, diff, declared_fns)` usa AST quando grammar disponível, fallback `vocabulary::scan` (camada `pattern` da W1) sobre o escopo do diff quando não — fail-open sempre; `extract_function_signatures(loader, source, lang_id)` extrai via query `.scm` ou regex agnóstico de fallback. Crates adicionados: `tree-sitter = "0.26"` e `tree-sitter-loader = "0.26"` apenas. **Nenhum** `tree-sitter-rust`/`tree-sitter-typescript`/`tree-sitter-javascript` — grammars individuais são instaladas pelo usuário. Benchmark `ast::bench` (parse + extract <50ms em arquivo médio quando grammar disponível). **Justificativa de existência da wave:** sem este módulo agnóstico, Camada 2 do gate (detecção de stub no diff) precisaria ou hardcodar grammars (violando [[feedback_mustard_agnostic]]) ou virar text-only (regressão desnecessária — perde precisão AST). Spec C estende o módulo com `extract_api_calls` + queries SOLID + queries por linguagem detectada. ACs binários: AC-A-2, AC-A-3 (via Camada 2 do gate W4), AC-A-16, AC-A-17.

### W2 — mustard-core-regression-check (core)

Arquivos: `packages/core/src/regression_check/mod.rs`, `packages/core/src/regression_check/snapshot.rs`, `packages/core/src/regression_check/compare.rs` (todos NOVO). Foto antes/depois primitive. Estruturas: `Snapshot::capture_for_spec(loader, spec_md, codebase) -> Snapshot` (recebe `GrammarLoader` da W1.5 — AST estrutural quando grammar disponível, bloco textual via regex+boundary matching quando não), `Snapshot::compare_to(other: &Snapshot) -> Diff` (diff AST estrutural ou `similar` text diff conforme fallback), `compare_snapshots(a, b) -> Vec<FunctionDelta>`. Serialização canônica JSON via `serde_json` (campos ordenados, bytes estáveis). Usa `similar = "2"` pra diff textual de corpo de função quando AST indisponível. Benchmark `regression_check::bench::compare_100_functions` (<50ms — AC-A-12). ACs binários: AC-A-4, AC-A-12.

### W3 — wave-summary-context-format (rt)

Arquivos: `apps/rt/src/run/wave_summary.rs`, `apps/rt/src/run/wave_context.rs`, `apps/cli/templates/skills/wave-summary-format.md` (todos NOVO). Schema do `_summary.md` com 7 seções obrigatórias (objetivo, herança, decisões, código, AC, verdict, próximos passos); schema do `_context.md` da wave N+1 (objetivo + herança + memória + posição no mapa + sugestão de próximos passos). Templates idempotentes via wikilinks. Geração atômica via `mustard_core::atomic_md::write_atomic`. ACs binários: AC-A-8, AC-A-9.

### W4 — gate-regression-check-run (rt)

Arquivos: `apps/rt/src/run/gate_regression_check.rs` (NOVO), `apps/rt/src/hooks/pre_edit_intent_check.rs` (NOVO opcional — alternativa run-based). Implementa os 3 momentos × 3 camadas: Momento 1 (pré-edit) lê o plano do agente + casa contra `vocabulary::scan` (W1); Momento 2 (durante o diff) constrói `GrammarLoader::from_project` e chama `ast::detect_stub_patterns(&loader, diff, declared_fns)` — AST exato quando a grammar da linguagem está instalada localmente, fallback `vocabulary::scan` da camada `pattern` (W1) sobre o escopo do diff quando não; Momento 3 (fechamento) chama `Snapshot::capture_for_spec(&loader, …)` + `compare_snapshots` antes e depois — diff estrutural via AST ou diff textual via `similar` conforme fallback. Veredict verde/amarelo/vermelho: verde passa, amarelo dispara AskUserQuestion (AC-A-6), vermelho bloqueia consolidação (AC-A-7). Grammar ausente localmente nunca causa panic — sempre fail-open com warning na telemetria. ACs binários: AC-A-1, AC-A-2, AC-A-3, AC-A-6, AC-A-7.

### W5 — span-level-integration (rt)

Arquivos: `apps/rt/src/hooks/subagent_inject.rs` (ESTENDIDO — herda da no-sqlite), `apps/rt/src/run/agent_prompt_render.rs` (ESTENDIDO — herda). Gate roda a cada `SubagentStop` via `gate_regression_check::check_after_child_return`; verdict por filho registrado em `_review-spans.md` (atômico, append-only); bloqueio de consolidação se algum filho retornou vermelho. ACs binários: AC-A-5.

### W6 — resume-bootstrap-disciplined (rt)

Arquivos: `apps/rt/src/run/resume_bootstrap.rs` (ESTENDIDO — herda da no-sqlite). Orçamento ≤10.000 tokens por bootstrap; pruning via wikilinks (carrega só os `_summary.md` cujos wikilinks aparecem no contexto da wave atual); `_context.md` da wave atual gera-se on-resume via `wave_context::build` (W3). ACs binários: AC-A-10.

### W7 — review-cobertura-w6 (mixed)

Roda a spec inteira contra a fixture do W6 (capturada em W0); mede quantos pontos do gate disparam (espera-se ≥3 dos 4 — AC-A-1); ajusta thresholds e vocabulário inicial baseado nos disparos reais. Sem arquivos novos — só configuração + relatório. ACs binários: AC-A-1, validação consolidada.

### W8 — qa-and-close-followups (mixed)

QA-functional roda todos os AC binários (AC-A-1 a AC-A-18) via `mustard-rt run qa-run`; quality-ledger ganha entrada inaugural com snapshot de métricas de baseline; emissão de `pipeline.status` Completed; CLOSE da spec A. Sem arquivos novos — só validação e fechamento.

### W8.5 — mustard-install-grammars (cli helper opcional)

Arquivos: `apps/cli/src/commands/install_grammars.rs` (NOVO). Subcomando opcional `mustard install-grammars` que lê o stack detectado em `detect_libs` (manifests do projeto-alvo) e imprime, para cada linguagem detectada, os repos canônicos do grammar tree-sitter (ex. `github.com/tree-sitter/tree-sitter-rust`) e o comando shell exato a rodar (`tree-sitter init && cd <grammar> && tree-sitter generate`). **Mustard não baixa, não clona, não compila** — só sugere. Output sempre fail-open; linguagem sem grammar canônico conhecido imprime "grammar não catalogado — buscar em tree-sitter.github.io". Sem dependências novas. ACs binários: AC-A-18.

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
- [feedback_mustard_agnostic](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->