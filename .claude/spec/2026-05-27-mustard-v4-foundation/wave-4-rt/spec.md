# Wave 4 — gate-regression-check-run (papel: rt)

### Stage: Analyze
### Outcome: Active
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Wave central da Spec A. Conecta as 3 primitivas de `mustard-core` (vocabulário W1, AST agnóstico W1.5, snapshot W2) num gate único com 3 momentos × 3 camadas. Momento 1 (pré-edit) lê o plano do agente + casa contra `vocabulary::scan` (W1) sobre o texto livre; Momento 2 (durante o diff) constrói `GrammarLoader::from_project` (W1.5) e chama `ast::detect_stub_patterns(&loader, diff, declared_fns)` — AST exato quando a grammar da linguagem está instalada, fallback `vocabulary::scan` da camada `pattern` sobre o escopo do diff quando não; Momento 3 (fechamento) chama `Snapshot::capture_for_spec(&loader, …)` + `compare_snapshots` antes e depois — diff AST estrutural ou diff textual via `similar` conforme o modo da captura. Veredict verde/amarelo/vermelho: verde passa, amarelo dispara AskUserQuestion (AC-A-6), vermelho bloqueia consolidação (AC-A-7). Grammar ausente nunca causa panic — sempre fail-open com warning na telemetria. Inclui hook opcional `pre_edit_intent_check` como alternativa run-based — registro em `apps/rt/src/hooks/registry.rs`.

## Arquivos tocados

- `apps/rt/src/run/gate_regression_check.rs` (NOVO) — `run`, `check_after_child_return`, types verdict
- `apps/rt/src/hooks/pre_edit_intent_check.rs` (NOVO opcional) — alternativa run-based ao Momento 1
- `apps/rt/src/hooks/registry.rs` (ESTENDIDO) — registra `pre_edit_intent_check` (gated por `MUSTARD_V4_BOOTSTRAP=0` para não conflitar com bootstrap mode)
- `apps/rt/src/run/mod.rs` (ESTENDIDO) — re-export do `gate_regression_check`
- `apps/rt/src/main.rs` (ESTENDIDO) — wiring de subcomando `mustard-rt run gate-regression-check --spec <name> --moment <1|2|3>`

## Funções tocadas

### Em `apps/rt/src/run/` (NOVO)
- `gate_regression_check::run`
- `gate_regression_check::check_after_child_return`

### Em `apps/rt/src/hooks/` (NOVO opcional)
- `pre_edit_intent_check::dispatch`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-1: Caso W6 reproduzido dispara o gate em ≥3 dos 4 pontos críticos (validado em W7)
- AC-A-2: Plano com `fail-open` ou `empurrar pra W…` dispara Momento 1
- AC-A-3: Diff com `fn X() -> Option<T> { None }` em função pública preservada dispara Momento 2
- AC-A-6: Verdict amarelo PERGUNTA (AskUserQuestion)
- AC-A-7: Verdict vermelho BLOQUEIA consolidação

## Tarefas

- [ ] T4.1: Criar `apps/rt/src/run/gate_regression_check.rs` com types verdict (verde/amarelo/vermelho) e a função `run` orquestrando os 3 momentos × 3 camadas
- [ ] T4.2: Implementar Momento 1 em `gate_regression_check::run` — lê plano do agente e casa contra `vocabulary::scan` (W1) (AC-A-2)
- [ ] T4.3: Implementar Momento 2 em `gate_regression_check::run` — constrói `GrammarLoader::from_project(spec.project_root())` (W1.5) e chama `ast::detect_stub_patterns(&loader, diff, declared_fns)` sobre funções declaradas como preservadas. Quando grammar disponível: detecção AST exata via queries `.scm`. Quando grammar não instalada: fallback automático `vocabulary::scan` da camada `pattern` (W1) sobre o escopo do diff. Fail-open sempre, nunca panic (AC-A-3)
- [ ] T4.4: Implementar `gate_regression_check::check_after_child_return` — Momento 3 chamando `Snapshot::capture_for_spec(&loader, …)` + `compare_snapshots` (W2) no fechamento, usando o mesmo `GrammarLoader` da T4.3. Resultado por função traz `mode: CaptureMode { Ast, Textual }` para o relatório no `_summary.md` (AC-A-1)
- [ ] T4.5: Implementar a classificação verdict — amarelo dispara `AskUserQuestion`, vermelho retorna erro bloqueante (AC-A-6, AC-A-7)
- [ ] T4.6: Criar `apps/rt/src/hooks/pre_edit_intent_check.rs` (opcional run-based) com `dispatch` cobrindo o Momento 1 e registrar em `apps/rt/src/hooks/registry.rs` gated por `MUSTARD_V4_BOOTSTRAP=0`
- [ ] T4.7: Estender `apps/rt/src/run/mod.rs` re-exportando `gate_regression_check` e `apps/rt/src/main.rs` com `mustard-rt run gate-regression-check --spec <name> --moment <1|2|3>`

## Dependências (waves anteriores)

- W1 (`vocabulary::scan` — usado no Momento 1 e como fallback no Momento 2)
- W1.5 (`GrammarLoader` agnóstico + `detect_stub_patterns` com fallback embutido)
- W2 (`Snapshot::capture_for_spec` recebendo o mesmo Loader)