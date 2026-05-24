# Tactical Fix: cache hit ratio honesta + legenda didática

## Contexto

Tactical fix derivado de [[2026-05-22-economia-didatica-e-economias-reais]].

A fórmula atual de cache hit em `packages/core/src/telemetry/reader.rs:309-326` e `:584-602`:

```
ratio = cache_read_input_tokens / (input_tokens + cache_read_input_tokens)
```

omite `cache_creation_input_tokens` (tokens escritos no cache pela 1ª vez, cobrados 1.25× ou 2× do preço). Resultado: quando um dispatch cria cache pela 1ª vez, esses tokens somem do denominador; dispatches subsequentes inflam o numerador → ratio agregada dispara pra 99.9% e mente sobre o custo real.

A Anthropic define `total_input_tokens = cache_read + cache_creation + input` ([docs](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)). Esse é o denominador honesto: "de todo o input que a API processou, quanto foi servido do cache".

Além do fix do cálculo: o card "Cache hit" em `apps/dashboard/src/pages/Economia.tsx` precisa de uma legenda didática explicando o que o número significa e por que importa (a Wave 3 já trocou a copy mas a legenda atual é vaga: "quanto do contexto a Anthropic reaproveitou — quanto maior, mais barato").

## Arquivos

- `packages/core/src/telemetry/reader.rs` — somar `cache_creation_input_tokens` no denominador de `cache_hit_ratio_permille` (linha 309) e `cache_hit_ratio_permille_for_spec` (linha 584). Atualizar doc-comments para refletir a fórmula nova.
- `apps/dashboard/src/pages/Economia.tsx` — legenda do card "Cache hit": explicar em 1-2 linhas o que o número mede (tokens servidos do cache ÷ tokens totais de input) e como interpretar (≥80% = cache funcionando; 50-80% = morno; <50% = prefixo está mudando muito).

## Tarefas

### Library Agent

- [x] `reader.rs:309-326` — `cache_hit_ratio_permille`: `SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(cache_read_input_tokens), 0), COALESCE(SUM(cache_creation_input_tokens), 0)`. Denominador = soma dos três.
- [x] `reader.rs:584-602` — `cache_hit_ratio_permille_for_spec`: mesma mudança.
- [x] Atualizar doc-comments para citar a fórmula da Anthropic: "cache_read ÷ (cache_read + cache_creation + input)".
- [x] Ajustar/adicionar testes: cenário com cache_creation > 0 deve produzir ratio < 100%.
- [x] `cargo build -p mustard-core && cargo test -p mustard-core --lib`.

### UI Agent

- [x] `Economia.tsx`: trocar `hint` do KPICard "Cache hit" para algo didático em 1-2 linhas. Exemplo: "tokens servidos do cache ÷ tokens totais de input. Acima de 80% é ótimo (a Anthropic só cobra 10% nesses tokens)."
- [x] Opcional: badge sutil de classificação visual abaixo do número (ex.: "ótimo" verde se ≥80%, "morno" âmbar 50-79%, "frio" cinza <50%) usando `<StatusDot>`/`MetricsPill` existentes; só se não quebrar o layout.
- [x] `pnpm --filter mustard-dashboard build`.

## Critérios de Aceitação

- [x] AC-1: build core verde — Command: `cargo build -p mustard-core`
- [x] AC-2: testes core verdes — Command: `cargo test -p mustard-core --lib`
- [x] AC-3: denominador inclui cache_creation — Command: `bash -c "test $(grep -c cache_creation_input_tokens packages/core/src/telemetry/reader.rs) -ge 2 && echo ok"`
- [x] AC-4: build dashboard verde — Command: `pnpm --filter mustard-dashboard build`
- [x] AC-5: legenda didática presente — Command: `bash -c "grep -q 'tokens servidos do cache' apps/dashboard/src/pages/Economia.tsx && echo ok"`

## Limites

- Não tocar schema (`run_usage` já tem `cache_creation_input_tokens`)
- Não mudar o nome do campo na API/DTO (`cache_hit_ratio_permille` permanece)
- Não remover o teste existente; só adicionar caso novo
- Surgical: só os 2 SELECTs em `reader.rs` + 1 hint no `Economia.tsx`
