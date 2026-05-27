# Wave 0 — analyze-and-fixtures (papel: mixed)

### Stage: Close
### Outcome: Completed
### Flags: 
### Scope: light
### Lang: pt-BR
### Parent: 2026-05-27-mustard-v4-foundation
### Checkpoint: 2026-05-27T17:56:09.926Z

## Contexto

Wave inaugural da Spec A. Entrega a fixture do caso W6 da no-sqlite (PICKUP em estado pré e pós), o parser do formato canônico de `## Funções tocadas` em `mustard-core`, e o fallback para specs legadas sem essa seção. Insumo bloqueador para W2/W4/W5/W7.

## Arquivos tocados

- `packages/core/src/spec/touched_functions.rs` (NOVO) — parser + `validate_touched_functions` + `functions_in_scope_with_fallback`
- `packages/core/src/spec/mod.rs` (ESTENDIDO) — re-export do `touched_functions`
- `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-pre/` (NOVO) — snapshot do `apps/dashboard/src-tauri/src/telemetry.rs` em estado pré-W6 (23 entradas no corpo)
- `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/w6-post/` (NOVO) — snapshot pós-W6 (0 entradas — stubs)
- `.claude/spec/2026-05-27-mustard-v4-foundation/fixtures/legacy-no-funcoes/spec.md` (NOVO) — fixture de spec legada sem `## Funções tocadas` para validar fallback

## Funções tocadas

### Em `packages/core/src/spec/` (NOVO)
- `spec::touched_functions::parse`
- `spec::touched_functions::validate_touched_functions`
- `spec::touched_functions::functions_in_scope_with_fallback`

## Acceptance Criteria

Subset relevante desta wave:
- AC-A-15: Spec sem `## Funções tocadas` → fallback usa funções públicas tocadas pelo diff; gate funciona sem panic em fixture legada
- AC-FT-1 a AC-FT-6 (definidos em `funcoes-tocadas.md` do design original) — parser aceita 3 formatos de qualificador, detecta status, valida unicidade, valida path_hint, fallback funciona, auto-validação da Spec A passa

## Tarefas

- [ ] T0.1: Criar `packages/core/src/spec/touched_functions.rs` com `parse` cobrindo as 3 variantes de qualificador (path_hint, módulo, função pura) (AC-FT-1)
- [ ] T0.2: Implementar `validate_touched_functions` no mesmo módulo — checa unicidade, status declarado e path_hint resolvível (AC-FT-2, AC-FT-3)
- [ ] T0.3: Implementar `functions_in_scope_with_fallback` que cai pra funções públicas tocadas pelo diff quando a seção está ausente (AC-A-15, AC-FT-5)
- [ ] T0.4: Estender `packages/core/src/spec/mod.rs` re-exportando `touched_functions`
- [ ] T0.5: Capturar fixture `w6-pre/` com snapshot de `apps/dashboard/src-tauri/src/telemetry.rs` em estado pré-W6 (23 entradas no corpo)
- [ ] T0.6: Capturar fixture `w6-post/` com o mesmo arquivo em estado pós-W6 (0 entradas — stubs) e fixture `legacy-no-funcoes/spec.md` sem a seção (AC-A-15)
- [ ] T0.7: Validar self-test rodando `validate_touched_functions` contra a própria Spec A — passa sem erros (AC-FT-6)

## Dependências (waves anteriores)

- Nenhuma (wave inaugural)