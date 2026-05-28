# Wave 2 — mustard-core-regression-check (papel: core)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Terceira primitiva de `mustard-core`. Entrega a foto antes/depois — captura o estado de cada função declarada em `## Funções tocadas`, compara dois snapshots e produz `Vec<FunctionDelta>`. Camada 3 do gate de regressão (W4) consome esta primitiva no Momento 3 (fechamento da wave). `Snapshot::capture_for_spec` recebe `GrammarLoader` (W1.5) como parâmetro: quando a grammar da linguagem do arquivo está instalada localmente, captura via AST (assinatura + corpo estrutural); quando não, fallback para extração textual (regex + boundary matching) com warning na telemetria. `Snapshot::compare_to` faz diff AST estrutural ou diff textual via `similar = "2"` conforme o modo da captura — fail-open sempre. Serialização canônica via `serde_json` (campos ordenados, bytes estáveis — diff reprodutível entre máquinas).

## Arquivos tocados

- `packages/core/src/regression_check/mod.rs` (NOVO) — types públicos (`Snapshot`, `Diff`, `FunctionDelta`)
- `packages/core/src/regression_check/snapshot.rs` (NOVO) — `Snapshot::capture_for_spec`
- `packages/core/src/regression_check/compare.rs` (NOVO) — `compare_snapshots` + `Snapshot::compare_to`
- `packages/core/Cargo.toml` (ESTENDIDO) — adiciona `similar = "2"` para diff de corpo de função
- `packages/core/src/lib.rs` (ESTENDIDO) — re-export do `regression_check`

## Funções tocadas

### Em `packages/core/src/regression_check/` (NOVO)
- `regression_check::Snapshot::capture_for_spec`
- `regression_check::Snapshot::compare_to`
- `regression_check::compare_snapshots`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-4: Foto antes/depois pega função que esvaziou (antes 23 entradas, depois 0) — via fixture W0
- AC-A-12: `compare_snapshots` compara 100 funções em <50ms (bench)

## Tarefas

- [ ] T2.1: Criar `packages/core/src/regression_check/mod.rs` com types públicos `Snapshot`, `Diff`, `FunctionDelta`
- [ ] T2.2: Implementar `regression_check::Snapshot::capture_for_spec(loader, spec_md, codebase)` em `packages/core/src/regression_check/snapshot.rs` lendo `## Funções tocadas` (W0). Para cada função declarada, resolve `lang_id` pela extensão+stack e tenta capturar corpo via `TreeSitterParser::for_language(&loader, lang_id)` (W1.5) — em caso de `Err(GrammarNotInstalled)`, cai para captura textual (regex+boundary matching) com warning na telemetria. Nunca panic
- [ ] T2.3: Implementar `regression_check::compare_snapshots` e `Snapshot::compare_to` em `packages/core/src/regression_check/compare.rs` produzindo `Vec<FunctionDelta>`. Para entradas capturadas via AST: diff estrutural por nó. Para entradas capturadas em modo textual: diff de linhas via `similar = "2"`. Resultado uniforme: `FunctionDelta` carrega `mode: CaptureMode { Ast, Textual }` (AC-A-4)
- [ ] T2.4: Estender `packages/core/Cargo.toml` adicionando `similar = "2"` (diff textual fallback) e re-exportar `regression_check` em `packages/core/src/lib.rs`
- [ ] T2.5: Garantir serialização canônica de `Snapshot` via `serde_json` com campos ordenados (diff reprodutível entre máquinas)
- [ ] T2.6: Adicionar teste rodando `capture_for_spec` + `compare_snapshots` contra fixtures `w6-pre/` e `w6-post/` (W0) — espera `FunctionDelta` registrando esvaziamento (23 → 0) (AC-A-4)
- [ ] T2.7: Adicionar bench de `compare_snapshots` validando limite de 100 funções em <50ms (AC-A-12)

## Dependências (waves anteriores)

- W0 (fixture pré/pós W6 + parser de `## Funções tocadas`)
- W1.5 (`GrammarLoader` + `TreeSitterParser` para extrair corpo de função via AST no `capture_for_spec`; quando grammar ausente, fallback textual interno)