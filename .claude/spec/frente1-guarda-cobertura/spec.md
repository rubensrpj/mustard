---
id: spec.frente1-guarda-cobertura
---

# Frente 1a — Guarda de cobertura de execução

<!-- drafter:tone=didactic -->

## Contexto

Doc-mãe: `docs/INTEGRIDADE-EXECUCAO-E-MEMORIA-UTIL.md` (Frente 1). Na Wave 2 do sialia um agente recebeu 10 arquivos no `## Files` (7 backend C# + 3 core TS), entregou só os 3 do core e **afirmou** que "o backend foi feito em paralelo" — inventou um agente inexistente. Ficaram faltando serviço/enum/endpoints/migration; AC quebrariam só no QA.

O Mustard já tem o sinal para pegar isso: cada arquivo prometido vira um item do `meta.json#checklist` da onda (`wave-scaffold`, `checklist_from_files`), e o hook `PostToolUse(Write|Edit)` marca `done:true` quando o arquivo é editado (`post_edit.rs`). Logo, **cobertura = todo item de arquivo do checklist está `done`**. Um prometido intacto permanece `done:false`.

## Métrica de sucesso

O harness detecta, ANTES do QA, quando uma onda deixou arquivos prometidos sem tocar, e (em `strict`) impede a onda de ser declarada "completa" — reabrindo-a — em vez de empurrar meia-vertical para o QA.

## Não-Objetivos

- Não é o despacho por subprojeto (Frente 1b, follow-up).
- Não é re-despachar automaticamente só o gap (annotation no wave-advance — follow-up).
- Não picotar ondas; a guarda não cria ondas, só reabre a mesma.

## Critérios de Aceitação

- **AC-1** — `wave_coverage::check` identifica arquivos prometidos e não tocados (unit)
  Command: `cargo test -p mustard-rt wave_coverage`
- **AC-2** — o portão de "onda completa" respeita o modo (strict bloqueia, warn/off passam)
  Command: `cargo test -p mustard-rt wave_complete`
- **AC-3** — suíte do rt verde
  Command: `cargo test -p mustard-rt`
- **AC-4** — lint limpo
  Command: `cargo clippy -p mustard-rt`

## Checklist

- [x] T1 — módulo `wave_coverage.rs`: `check(wave_dir) -> CoverageVerdict{ok,missing}` (prometidos = itens de arquivo do `meta.json#checklist`; falta = `done:false`) + `mode()` de `MUSTARD_WAVE_COVERAGE_MODE` (Off/Warn/Strict, padrão Warn) + `blocks(verdict,mode)` (política pura: só strict bloqueia).
- [x] T2 — amarrado no `wave_complete_observer`: após `wave_is_complete`, roda a cobertura; strict → não emite `wave.complete` (onda reabre); warn → avisa e segue; off → ignora.
- [x] T3 — testes (AC-1: `wave_coverage`; AC-2: `wave_complete`) + suíte verde (3107) + clippy limpo.