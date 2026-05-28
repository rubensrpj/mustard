# Wave 1.5 — mustard-core-ast-loader-dynamic (papel: core)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Segunda primitiva de `mustard-core`. Entrega o módulo `ast` **agnóstico desde o nascimento** — zero match hardcoded de linguagem. Camada 2 do gate de regressão (W4) precisa detectar padrões de stub no diff (`fn X() -> Option<T> { None }` em função pública declarada como preservada); sem AST, a detecção fica restrita a vocabulary scan textual. Versão anterior desta wave entregava `TreeSitterParser::for_language` com `match lang { "rust" => …, "typescript" => …, "javascript" => …, }` — débito agnóstico que violava [[feedback_mustard_agnostic]] (primícia reforçada pelo usuário em 2026-05-27). O redesign v2 mantém o módulo `ast::*` na Spec A mas resolve grammars via `tree_sitter::Loader` (lib oficial da família tree-sitter): descobre grammars instaladas pelo usuário em `~/.config/tree-sitter/config.json` em runtime, filtradas pelo stack detectado pelo `detect_libs` do projeto-alvo. Linguagem detectada sem grammar instalada → warning na telemetria + fail-open (Camada 2 cai para `vocabulary::scan` da W1 sobre o escopo do diff). Nunca panic. Quando grammar disponível, precisão AST exata; quando não, precisão vocabulary textual.

**Caveat de naming:** este diretório usa `wave-1_5-core/` (underscore) porque o parser `parse_wave_dir_number` em `apps/rt/src/run/dependency_precheck.rs:855` extrai dígitos contíguos após `wave-` e para na primeira não-dígito — tanto `wave-1.5-core/` quanto `wave-1_5-core/` resolvem para wave=1 e colidem com `wave-1-core/`. A escolha do underscore evita fricção em ferramentas que confundem ponto com extensão. Tooling downstream que precisar distinguir W1 de W1.5 deve consumir o nome literal do diretório, não o número parseado. Fix do parser (`\d+(_\d+)?`) fica como follow-up — não bloqueia esta spec.

## Arquivos tocados

- `packages/core/src/ast/mod.rs` (NOVO) — types públicos (`Tree`, `FunctionSig`, `StubMatch`, `AstError`)
- `packages/core/src/ast/loader.rs` (NOVO) — `GrammarLoader::from_project`, `GrammarLoader::language`
- `packages/core/src/ast/parser.rs` (NOVO) — `TreeSitterParser::for_language`, `TreeSitterParser::parse`
- `packages/core/src/ast/queries.rs` (NOVO) — `QuerySet::load_for` carregando `.scm` de `.claude/grammars/{lang_id}/queries/*.scm`
- `packages/core/src/ast/stub_detect.rs` (NOVO) — `detect_stub_patterns(loader, diff, declared_fns)` com fallback vocabulary scan
- `packages/core/src/ast/signature.rs` (NOVO) — `extract_function_signatures(loader, source, lang_id)` com fallback regex
- `packages/core/Cargo.toml` (ESTENDIDO) — `tree-sitter = "0.22"` e `tree-sitter-loader = "0.22"`; **zero** grammars individuais
- `packages/core/src/lib.rs` (ESTENDIDO) — re-export do módulo `ast`

## Funções tocadas

### Em `packages/core/src/ast/` (NOVO — agnóstico via Loader)
- `ast::GrammarLoader::from_project`
- `ast::GrammarLoader::language`
- `ast::TreeSitterParser::for_language`
- `ast::TreeSitterParser::parse`
- `ast::QuerySet::load_for`
- `ast::detect_stub_patterns`
- `ast::extract_function_signatures`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-16: `detect_stub_patterns` detecta os 5 padrões (`None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()`) em função pública declarada como preservada — via queries `.scm` resolvidas pelo `GrammarLoader` quando grammar disponível; fallback `vocabulary::scan` (W1, camada `pattern`) sobre o escopo do diff quando grammar não instalada; fail-open, nunca panic
- AC-A-17: `GrammarLoader::from_project(root)` resolve dinamicamente grammars instaladas em `~/.config/tree-sitter/config.json` via `tree_sitter_loader::Loader::find_all_languages`, filtradas pelo stack detectado em `detect_libs`. Linguagem detectada sem grammar → warning + fail-open. **Zero match hardcoded de linguagem no código**

## Tarefas

- [ ] T1.5.1: Criar `packages/core/src/ast/mod.rs` com types públicos `Tree`, `FunctionSig`, `StubMatch`, `AstError` (variantes: `GrammarNotInstalled(String)`, `ParseFailed`, `QueryLoadFailed(PathBuf)`)
- [ ] T1.5.2: Implementar `ast::GrammarLoader::from_project(project_root)` em `packages/core/src/ast/loader.rs`: chama `tree_sitter_loader::Loader::find_all_languages` sobre o config default (`~/.config/tree-sitter/config.json`), filtra pelo stack vindo de `detect_libs`, popula `HashMap<String, Language>` com as grammars efetivamente disponíveis. Implementar `language(lang_id) -> Option<Language>` (NUNCA panic, NUNCA hardcode de id) (AC-A-17)
- [ ] T1.5.3: Implementar `ast::TreeSitterParser::for_language(loader, lang_id)` em `packages/core/src/ast/parser.rs` — delega a resolução de `Language` ao `loader.language(lang_id)`; retorna `Err(AstError::GrammarNotInstalled)` quando ausente, sem `match` interno. Implementar `parse(source) -> Result<Tree, AstError>` (AC-A-17)
- [ ] T1.5.4: Implementar `ast::QuerySet::load_for(lang_id)` em `packages/core/src/ast/queries.rs` carregando arquivos `.scm` de `.claude/grammars/{lang_id}/queries/*.scm` no projeto-alvo. Ausência do diretório → `QuerySet::default()` (vazio), não erro
- [ ] T1.5.5: Implementar `ast::detect_stub_patterns(loader, diff, declared_fns)` em `packages/core/src/ast/stub_detect.rs`: para cada arquivo do diff, resolve `lang_id` pela extensão+stack; se `loader.language(lang_id).is_some()` usa `QuerySet` + tree-sitter para casar os 5 padrões em corpos de função pública declarada; senão chama `vocabulary::scan` da camada `pattern` (W1) sobre o escopo textual do diff. Retorna `Vec<StubMatch>`. Fail-open: nunca panic, sempre alguma detecção (AC-A-16)
- [ ] T1.5.6: Implementar `ast::extract_function_signatures(loader, source, lang_id)` em `packages/core/src/ast/signature.rs`: via query `.scm` quando grammar disponível; senão regex agnóstico (linha começando por `pub fn`, `pub async fn`, `export function`, `def `, etc.) — fallback explícito documentado como impreciso
- [ ] T1.5.7: Estender `packages/core/Cargo.toml` adicionando `tree-sitter = "0.26"` e `tree-sitter-loader = "0.26"`. **Não** adicionar `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`, nem qualquer grammar individual ([[feedback_mustard_agnostic]]). Estender `packages/core/src/lib.rs` com `pub mod ast;`
- [ ] T1.5.8: Adicionar teste `ast::loader::test_agnostic_discovery_and_missing_grammar_fail_open` — monta um `Loader` apontando para diretório temporário (sem grammars), verifica que `from_project` retorna `GrammarLoader` válido com `available_languages` vazio, `language("rust")` retorna `None`, `TreeSitterParser::for_language` retorna `Err(AstError::GrammarNotInstalled("rust"))` sem panic (AC-A-17)
- [ ] T1.5.9: Adicionar teste `ast::stub_detect::test_detect_all_patterns_with_fallback` rodando contra a fixture pós-W6 (W0): valida que detecta `None`/`vec![]`/`Default::default()`/`unimplemented!()`/`todo!()` quando grammar Rust está disponível e que cai para fallback `vocabulary::scan` quando grammar é removida do `Loader` (mockada via `GrammarLoader::empty()` helper de teste). Em ambos os modos: ≥1 detecção em cada padrão (AC-A-16)

## Dependências (waves anteriores)

- W0 (fixture do estado pré/pós-W6 + parser de `## Funções tocadas`, usado como input dos testes)
- W1 (`mustard_core::vocabulary` — camada `pattern` consumida pelo fallback de `detect_stub_patterns`)

<!-- wikilinks-footer-start -->
- [feedback_mustard_agnostic](?) ⚠ não resolvido
<!-- wikilinks-footer-end -->