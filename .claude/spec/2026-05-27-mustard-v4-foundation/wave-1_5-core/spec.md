# Wave 1.5 — mustard-core-ast-minimal (papel: core)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Segunda primitiva de `mustard-core`. Entrega o subset mínimo do módulo `ast` para a Spec A — apenas o suficiente para a Camada 2 do gate de regressão detectar padrões de stub no diff (sem AST não há detecção de `fn X() -> Option<T> { None }`). Suporta apenas 3 linguagens (`rust`, `typescript`, `javascript`); grammars adicionais (Python, Go, C#, Java) ficam pra Spec C. Linguagem desconhecida retorna `Err` sem panic (AC-A-17). Sem este subset, M9 + M14 ficam sem braço de execução.

**Caveat de naming:** este diretório usa `wave-1_5-core/` (underscore) porque o parser `parse_wave_dir_number` em `apps/rt/src/run/dependency_precheck.rs:855` extrai dígitos contíguos após `wave-` e para na primeira não-dígito — tanto `wave-1.5-core/` quanto `wave-1_5-core/` resolvem para wave=1 e colidem com `wave-1-core/`. A escolha do underscore evita fricção em ferramentas que confundem ponto com extensão. Tooling downstream que precisar distinguir W1 de W1.5 deve consumir o nome literal do diretório, não o número parseado. Fix do parser (`\d+(_\d+)?`) fica como follow-up — não bloqueia esta spec.

## Arquivos tocados

- `packages/core/src/ast/mod.rs` (NOVO) — types públicos (`Tree`, `FunctionSig`, `StubMatch`)
- `packages/core/src/ast/parser.rs` (NOVO) — `TreeSitterParser::for_language` + `parse`
- `packages/core/src/ast/signature.rs` (NOVO) — `extract_function_signatures`
- `packages/core/src/ast/stub_detect.rs` (NOVO) — `detect_stub_patterns` (corpo `None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()`)
- `packages/core/Cargo.toml` (ESTENDIDO) — `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`

## Funções tocadas

### Em `packages/core/src/ast/` (NOVO — subset mínimo)
- `ast::TreeSitterParser::for_language`
- `ast::TreeSitterParser::parse`
- `ast::extract_function_signatures`
- `ast::detect_stub_patterns`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-16: `detect_stub_patterns` detecta os 5 padrões (`None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()`) em função pública declarada como preservada
- AC-A-17: `TreeSitterParser::for_language` aceita `rust`, `typescript`, `javascript` sem panic; desconhecida retorna `Err` sem panic

## Tarefas

- [ ] T1.5.1: Criar `packages/core/src/ast/mod.rs` com types públicos `Tree`, `FunctionSig`, `StubMatch`
- [ ] T1.5.2: Implementar `ast::TreeSitterParser::for_language` aceitando apenas `rust`, `typescript`, `javascript` e retornando `Err` em linguagem desconhecida (AC-A-17)
- [ ] T1.5.3: Implementar `ast::TreeSitterParser::parse` em `packages/core/src/ast/parser.rs` produzindo `Tree`
- [ ] T1.5.4: Implementar `ast::extract_function_signatures` em `packages/core/src/ast/signature.rs` extraindo `FunctionSig` por linguagem
- [ ] T1.5.5: Implementar `ast::detect_stub_patterns` em `packages/core/src/ast/stub_detect.rs` cobrindo os 5 padrões (`None`, `vec![]`, `Default::default()`, `unimplemented!()`, `todo!()`) (AC-A-16)
- [ ] T1.5.6: Estender `packages/core/Cargo.toml` adicionando `tree-sitter`, `tree-sitter-rust`, `tree-sitter-typescript`, `tree-sitter-javascript`
- [ ] T1.5.7: Adicionar teste de `detect_stub_patterns` rodando contra a fixture pós-W6 (W0) — confirma os 5 padrões em função pública declarada como preservada (AC-A-16)

## Dependências (waves anteriores)

- W0 (fixture do estado pré-W6, usada como input do parser nos testes)