# Fixture — spec legada sem `## Funções tocadas`

### Stage: Execute
### Outcome: Active
### Flags:
### Scope: light
### Lang: pt-BR

## Contexto

Esta é uma fixture controlada para validar **AC-A-15**: o gate de regressão
deve funcionar (sem panic) quando uma spec não declara a seção canônica
`## Funções tocadas`. O resolver
[`functions_in_scope_with_fallback`](../../../../packages/core/src/spec/touched_functions.rs)
deve detectar a ausência e invocar a closure de fallback fornecida pelo
chamador — tipicamente "funções públicas tocadas pelo diff" derivadas da W1.5
(AST).

Especificamente:

- `parse(spec_md)` retorna `None` para este conteúdo (seção ausente).
- `functions_in_scope_with_fallback(spec_md, || diff_fns)` retorna
  `Resolution::Fallback(diff_fns)` — a closure de fallback é invocada
  exatamente uma vez.
- `validate_touched_functions` não é chamada (não há `TouchedFunctions`
  parseado).

Reproduz o cenário onde uma spec antiga (pré-Mustard-v4) cai no gate sem ter
a declaração canônica. O fallback é o caminho fail-open: a wave segue,
mas a cobertura de AC-typing (Fase B) e o snapshot W2 cairão para o conjunto
derivado do diff em vez do conjunto declarado.

## Usuários/Stakeholders

Maintainer único (Rubens) revisando uma spec pré-v4 antes do bootstrap do
gate. Caso real: 30+ specs no `~/.mustard-backups/2026-05-25-specs-archive/`
não têm a seção e poderiam disparar regressão se a v4 fosse aplicada
retroativamente.

## Métrica de sucesso

Quando o caso é reproduzido contra o gate desta spec A:

- AC-A-15 (binário): `parse(legacy_md) == None` e
  `functions_in_scope_with_fallback(legacy_md, closure)` invoca a closure.
- Nenhum panic, nenhuma asserção falha; verdict do gate é "amarelo" (avisa
  que o fallback foi usado) ou "verde" (se a wave for tagged como legacy).

## Não-Objetivos

- Não tenta cobrir o caso onde a seção existe mas está **mal formada**;
  esse caso vai pra fixture separada e cai em `validate_touched_functions`.
- Não tenta cobrir specs sem cabeçalho de lifecycle — separado, foco em
  `parse_state`.

## Critérios de Aceitação

- [ ] FX-LEGACY-1: `parse(content)` retorna `None`.
  Command: `bash -c 'cd /c/Atiz/mustard && cargo test -p mustard-core spec::touched_functions::fallback_runs_closure_when_section_absent'`
- [ ] FX-LEGACY-2: `functions_in_scope_with_fallback(content, closure)` chama
  a closure exatamente uma vez e retorna `Resolution::Fallback`.
  Command: idem (mesmo teste cobre).

## Tarefas

Não aplicável — fixture estática.

## Notas

O conteúdo abaixo desta seção é deliberadamente prosa livre. Inclui menções
à string `## Funções tocadas` **dentro de um bloco de código** para garantir
que o parser respeita fences (regra `is_section_open` + `in_fence`).

```text
## Funções tocadas

### Em `crate::fake/` (NOVO)
- `fake::should_not_be_parsed`
```

A linha acima está num fence — `parse` deve ignorar.

Também menciona em uma `## ` posterior que esta wave **não** declara funções
tocadas, garantindo que nenhum outro `##` heading seja confundido com a
seção canônica.