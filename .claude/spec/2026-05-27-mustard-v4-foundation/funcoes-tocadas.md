# Funções tocadas — Spec A v4

> **Mantra:** fonte primária é o design original em `.claude/plans/mustard-v4/05-design/funcoes-tocadas.md` (§ formato canônico) e em `.claude/plans/mustard-v4/06-specs/spec-A-foundation.md` (linhas 155-193). Espelho local desta spec para consumo pelo `spec-validate` (auto-validação AC-FT-6).

> Parent: [`spec.md`](./spec.md) — esta é a declaração de escopo de funções públicas tocadas pela Spec A. Cruzamento futuro com `## Acceptance Criteria` tipado (Fase B) e com diff real (Fase D / `qa_coverage`).

## Formato

Seção espelha o que aparece em `spec.md → ## Funções tocadas`, conforme regras R1-R6 do formato canônico:

| Regra | Definição |
|---|---|
| **R1** | Cada subseção começa com `### Em \`{path}\` ({status})` onde status ∈ `NOVO` / `ESTENDIDO` / `MODIFICADO` |
| **R2** | Cada linha de função começa com `- ` seguido de qualificador |
| **R3** | Qualificador em 3 formatos: `crate::module::function`, `module::function`, ou `path/to/file::function` |
| **R4** | Comentários `— justificativa` permitidos após o qualificador, separados por `—` (em dash) |
| **R5** | Apenas **funções públicas** declaradas (não privadas, não tests). Helpers internos saem do escopo |
| **R6** | `NOVO` = função não existe ainda. `ESTENDIDO` = existe e ganha responsabilidade nova. `MODIFICADO` = comportamento muda materialmente |

## Em `packages/core/src/vocabulary/` (NOVO — W1)

- `vocabulary::VocabularyMatcher::from_layers` — constrói o matcher a partir das 4 camadas (semantic/pattern/keyword/noise) lidas de `.claude/vocab/regression.toml`
- `vocabulary::VocabularyMatcher::scan` — varre um texto e retorna matches com camada + peso
- `vocabulary::VocabLayer::parse_from_toml` — deserializa uma camada do TOML

## Em `packages/core/src/ast/` (NOVO — W1.5, agnóstico via Loader)

- `ast::GrammarLoader::from_project` — descobre grammars instaladas pelo usuário em `~/.config/tree-sitter/config.json` via `tree_sitter_loader::Loader::find_all_languages`, filtradas pelo stack detectado em `detect_libs`. Zero match hardcoded de linguagem
- `ast::GrammarLoader::language` — `Option<Language>` por id; `None` quando grammar não instalada (fail-open, nunca panic)
- `ast::TreeSitterParser::for_language` — fábrica do parser delegando a resolução de `Language` ao Loader; sem `match` interno
- `ast::TreeSitterParser::parse` — gera `Tree` a partir de string-fonte
- `ast::QuerySet::load_for` — carrega queries `.scm` de `.claude/grammars/{lang_id}/queries/*.scm`; ausência do diretório → `QuerySet::default()` vazio
- `ast::detect_stub_patterns` — detecta corpo `None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()` em funções públicas declaradas como preservadas; AST quando grammar disponível, fallback `vocabulary::scan` da camada `pattern` (W1) sobre o escopo do diff quando não
- `ast::extract_function_signatures` — extrai `FunctionSig` via query `.scm` quando grammar disponível, fallback regex agnóstico

## Em `packages/core/src/regression_check/` (NOVO — W2)

- `regression_check::Snapshot::capture_for_spec` — captura o snapshot do estado atual das funções declaradas em `## Funções tocadas` da spec
- `regression_check::Snapshot::compare_to` — compara dois snapshots e retorna `Diff` por função
- `regression_check::compare_snapshots` — função de conveniência que casa dois `Snapshot` e produz `Vec<FunctionDelta>`

## Em `packages/core/src/spec/` (NOVO — formato canônico, W0)

- `spec::touched_functions::parse` — parser do markdown `## Funções tocadas` retornando `TouchedFunctions { added, extended, modified }` (campos EN; labels markdown NOVO/ESTENDIDO/MODIFICADO mapeados para `Status::{Added,Extended,Modified}` pelo parser)
- `spec::touched_functions::validate_touched_functions` — valida unicidade de qualifier, existência de path_hint, e checa NOVO contra codebase
- `spec::touched_functions::functions_in_scope_with_fallback` — quando seção ausente, fallback para funções públicas do diff (AC-A-15)

## Em `apps/rt/src/run/` (NOVO — W3, W4)

- `gate_regression_check::run` — orquestra os 3 momentos × 3 camadas; retorna verdict verde/amarelo/vermelho
- `gate_regression_check::check_after_child_return` — entry-point para span-level eval no hook `subagent_inject` (W5)
- `wave_summary::build` — gera o conteúdo do `_summary.md` com 7 seções obrigatórias
- `wave_summary::write` — escreve atomicamente via `atomic_md::write_atomic`
- `wave_context::build` — gera o `_context.md` da wave N+1 a partir dos `_summary.md` das anteriores
- `wave_context::write` — write atômico

## Em `apps/rt/src/run/` (ESTENDIDO — herdado da no-sqlite, W5/W6)

- `resume_bootstrap::run` — adiciona pruning por orçamento ≤10k tokens (W6); pruning via wikilinks dos `_summary.md` mencionados no contexto da wave atual
- `agent_prompt_render::run` — adiciona injection de vocabulário pré-armado no prompt do agente (W5)

## Em `apps/rt/src/hooks/` (ESTENDIDO + NOVO opcional, W4/W5)

- `subagent_inject::dispatch` — ESTENDIDO: adiciona vocabulário ao prompt + dispara `gate_regression_check::check_after_child_return` a cada `SubagentStop` (W5)
- `pre_edit_intent_check::dispatch` — NOVO opcional (W4): alternativa run-based ao gate Momento 1; lê plano do agente em `PreToolUse(Edit)` e casa contra vocabulário antes da edit acontecer

## Notas sobre R1-R6

- **R5 enforçado:** todas as funções listadas acima são `pub fn` no Rust gerado (módulo `mod.rs` re-exporta). Helpers internos (`fn parse_inner`, `fn build_section`) não entram no escopo.
- **R6 — distinção crítica:** `subagent_inject::dispatch` aparece como ESTENDIDO porque a função já existe na no-sqlite (apenas ganha lógica adicional de span-level), enquanto `gate_regression_check::run` é NOVO (módulo inexistente).
- **R4 — comentários em-dash:** usados acima para justificar entradas complexas (ex.: `GrammarLoader::from_project` documentando a descoberta agnóstica de grammars em runtime).
- **Cruzamento futuro com AC tipado (Fase B):** cada função listada aqui em estado NOVO ou MODIFICADO **exigirá** ≥1 AC positivo + ≥1 AC negativo; ESTENDIDO exige ≥1 AC positivo + ≥1 AC não-regressão; função em diff fora desta seção → P0 de cobertura. Definido em `ac-typed.md` (Fase B), implementado em `qa_coverage` (Fase D).

## Auto-validação (AC-FT-6)

Esta `spec.md` cumpre todos os critérios de `funcoes-tocadas.md`:
- Seção `## Funções tocadas` presente na `spec.md` (R1 — formato `### Em \`path\` (STATUS)`)
- Cada linha começa com `- ` + qualificador (R2)
- Qualificadores em formato `module::function` (R3)
- Apenas funções públicas declaradas (R5)
- Status NOVO/ESTENDIDO consistente entre `spec.md` e este espelho (R6)

Validação executável em W0: `cargo test -p mustard-core spec::touched_functions::test_validate_spec_a_self`.